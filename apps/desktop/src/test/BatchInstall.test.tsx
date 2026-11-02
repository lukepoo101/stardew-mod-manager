import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BatchInstall } from "@/features/mods/BatchInstall";
import { api } from "@/shared/api/client";
import type { OperationPreviewDto } from "@/shared/api/generated";
import { classifyBatch, installOrder } from "@/shared/mods/batch";

afterEach(() => vi.restoreAllMocks());

const preview = (
  id: string,
  over: Partial<OperationPreviewDto> = {},
): OperationPreviewDto => ({
  operation_id: `op-${id}`,
  artifact_hash: `hash-${id}`,
  original_filename: `${id}.zip`,
  byte_size: 1,
  detected_components: [
    {
      unique_id: id,
      name: id,
      author: "a",
      version: "1.0.0",
      description: null,
      relative_root: id,
      requirements: [],
    },
  ],
  dependencies_satisfied: true,
  warnings: [],
  blockers: [],
  affected_profile_component_ids: [],
  other_blockers: [],
  expected_profile_revision: null,
  files: [],
  replaces: [],
  ...over,
});

/**
 * An archive with one mod that needs others, as the backend reports it:
 * each requirement with its status, and a blocker for each unmet one.
 */
const needing = (
  id: string,
  needs: [string, string | null, "missing" | "too_old"][],
  version = "1.0.0",
): OperationPreviewDto => {
  const base = preview(id);
  return {
    ...base,
    detected_components: [
      {
        ...base.detected_components[0],
        version,
        requirements: needs.map(([unique_id, minimum, status]) => ({
          unique_id,
          name: status === "too_old" ? unique_id : null,
          installed_version: status === "too_old" ? "0.1.0" : null,
          minimum_version: minimum,
          kind: "required",
          status,
        })),
      },
    ],
    dependencies_satisfied: needs.length === 0,
    blockers: needs.map(([need]) => `${id} needs ${need}.`),
  };
};

const order = (entries: ReturnType<typeof classifyBatch>) =>
  installOrder(entries)
    .filter((e) => e.status !== "blocked")
    .map((e) => e.item.path.replace("/dl/", ""));

describe("ordering a batch", () => {
  it("installs a chain bottom-up whatever order it was chosen in", () => {
    const entries = classifyBatch([
      {
        path: "/dl/chainA.zip",
        preview: needing("Chain.Top", [["Chain.Middle", null, "missing"]]),
      },
      {
        path: "/dl/chainB.zip",
        preview: needing("Chain.Middle", [["Chain.Core", "2.0.0", "missing"]]),
      },
      { path: "/dl/other.zip", preview: preview("Other") },
      {
        path: "/dl/chainC.zip",
        preview: needing("Chain.Core", [], "3.0.0"),
      },
    ]);
    expect(entries.map((e) => e.status)).toEqual([
      "after_others",
      "after_others",
      "ready",
      "ready",
    ]);
    expect(order(entries)).toEqual([
      "other.zip",
      "chainC.zip",
      "chainB.zip",
      "chainA.zip",
    ]);
    expect(entries[1].reason).toMatch(
      /Chain\.Middle needs Chain\.Core 2\.0\.0 or newer: Chain\.Core 3\.0\.0 from chainC\.zip is installed first/,
    );
  });

  it("does not promise a version the batch cannot provide", () => {
    const entries = classifyBatch([
      { path: "/dl/frame.zip", preview: needing("SpaceCore", [], "1.25.0") },
      {
        path: "/dl/toonew.zip",
        preview: needing("Picky", [["SpaceCore", "9.0.0", "missing"]]),
      },
      {
        path: "/dl/fan.zip",
        preview: needing("Picky.Fan", [["Picky", null, "missing"]]),
      },
    ]);
    expect(entries.map((e) => e.status)).toEqual([
      "ready",
      "blocked",
      "blocked",
    ]);
    expect(entries[1].reason).toBe(
      "Cannot install: Picky needs SpaceCore 9.0.0 or newer; the batch has 1.25.0 (frame.zip).",
    );
    expect(entries[2].reason).toMatch(/needs toonew\.zip, which cannot be/);
  });

  it("reports archives that need each other instead of guessing an order", () => {
    const entries = classifyBatch([
      { path: "/dl/a.zip", preview: needing("A", [["B", null, "missing"]]) },
      { path: "/dl/b.zip", preview: needing("B", [["A", null, "missing"]]) },
      { path: "/dl/c.zip", preview: needing("C", [["A", null, "missing"]]) },
      { path: "/dl/d.zip", preview: preview("D") },
    ]);
    expect(entries.map((e) => e.status)).toEqual([
      "blocked",
      "blocked",
      "blocked",
      "ready",
    ]);
    expect(entries[0].reason).toMatch(
      /need each other.*a\.zip → b\.zip → a\.zip/,
    );
    expect(entries[2].reason).toMatch(
      /needs a\.zip, which cannot be installed/,
    );
  });

  it("puts an update that a mod needs before that mod", () => {
    const upgrade = needing("Core", [], "3.0.0");
    upgrade.replaces = [
      {
        profile_component_id: "c",
        unique_id: "Core",
        name: "Core",
        installed_version: "1.0.0",
        incoming_version: "3.0.0",
        direction: "upgrade",
        stops_loading: [],
      },
    ];
    upgrade.blockers = [
      "A mod with this UniqueID is already installed in this profile: Core.",
    ];
    upgrade.other_blockers = upgrade.blockers;
    const entries = classifyBatch([
      {
        path: "/dl/user.zip",
        preview: needing("User", [["Core", "2.0.0", "too_old"]]),
      },
      { path: "/dl/core.zip", preview: upgrade },
    ]);
    expect(entries.map((e) => e.status)).toEqual(["after_others", "upgrade"]);
    expect(order(entries)).toEqual(["core.zip", "user.zip"]);
  });
});

describe("classifying a batch", () => {
  it("says what happens to each archive before anything changes", () => {
    const entries = classifyBatch([
      { path: "/dl/Lib.zip", preview: preview("Z.Lib") },
      {
        path: "/dl/Needy.zip",
        preview: needing("A.Needy", [["Z.Lib", null, "missing"]]),
      },
      {
        path: "/dl/Orphan.zip",
        preview: needing("A.Orphan", [["Gone.Mod", null, "missing"]]),
      },
      { path: "/dl/Lib-copy.zip", preview: preview("z.lib") },
      {
        path: "/dl/Old.zip",
        preview: preview("O.Mod", {
          blockers: [
            "A mod with this UniqueID is already installed in this profile",
          ],
          files: [],
          replaces: [
            {
              profile_component_id: "c",
              unique_id: "O.Mod",
              name: "O.Mod",
              installed_version: "2.0.0",
              incoming_version: "1.0.0",
              direction: "downgrade",
              stops_loading: [],
            },
          ],
        }),
      },
      {
        path: "/dl/New.zip",
        preview: preview("N.Mod", {
          blockers: [
            "A mod with this UniqueID is already installed in this profile",
          ],
          files: [],
          replaces: [
            {
              profile_component_id: "d",
              unique_id: "N.Mod",
              name: "N.Mod",
              installed_version: "1.0.0",
              incoming_version: "2.0.0",
              direction: "upgrade",
              stops_loading: [],
            },
          ],
        }),
      },
      { path: "/dl/broken.zip", error: "No manifest.json found" },
    ]);
    expect(entries.map((e) => e.status)).toEqual([
      "ready",
      "after_others",
      "blocked",
      "duplicate",
      "downgrade",
      "upgrade",
      "failed",
    ]);
    expect(entries[3].reason).toMatch(/same mod as Lib\.zip/);
    expect(entries[6].reason).toBe("No manifest.json found");
  });
});

describe("installing a batch", () => {
  it("inspects each archive, then installs what others need first", async () => {
    const inspect = vi
      .spyOn(api, "inspectPackageForInstall")
      .mockImplementation(async (path) =>
        path.includes("Needy")
          ? needing(
              "A.Needy",
              inspect.mock.calls.length > 2 ? [] : [["Z.Lib", null, "missing"]],
            )
          : preview("Z.Lib"),
      );
    vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const execute = vi
      .spyOn(api, "executeOperation")
      .mockResolvedValue({} as never);
    render(
      <BatchInstall
        profileId="p1"
        paths={["/dl/Needy.zip", "/dl/Lib.zip"]}
        onClose={vi.fn()}
      />,
    );
    expect(
      await screen.findByText(
        /Z\.Lib 1\.0\.0 from Lib\.zip is installed first/,
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Install 2" }));
    expect(await screen.findByText("All 2 installed.")).toBeInTheDocument();
    // Lib before Needy, each freshly prepared.
    expect(execute.mock.calls.map((c) => c[0])).toEqual([
      "op-Z.Lib",
      "op-A.Needy",
    ]);
  });

  it("does not try a mod whose requirement failed to install", async () => {
    vi.spyOn(api, "inspectPackageForInstall").mockImplementation(
      async (path) =>
        path.includes("Needy")
          ? needing("A.Needy", [["Z.Lib", null, "missing"]])
          : preview("Z.Lib"),
    );
    vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const execute = vi
      .spyOn(api, "executeOperation")
      .mockRejectedValue(new Error("Disk full"));
    render(
      <BatchInstall
        profileId="p1"
        paths={["/dl/Needy.zip", "/dl/Lib.zip"]}
        onClose={vi.fn()}
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Install 2" }));
    expect(
      await screen.findByText(
        /Not installed: it needs Lib\.zip, which was not installed/,
      ),
    ).toBeInTheDocument();
    expect(execute).toHaveBeenCalledTimes(1);
  });

  it("leaves a removed archive out", async () => {
    vi.spyOn(api, "inspectPackageForInstall").mockImplementation(async (path) =>
      preview(path.includes("One") ? "One" : "Two"),
    );
    vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const execute = vi
      .spyOn(api, "executeOperation")
      .mockResolvedValue({} as never);
    render(
      <BatchInstall
        profileId="p1"
        paths={["/dl/One.zip", "/dl/Two.zip"]}
        onClose={vi.fn()}
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Remove Two.zip from the batch",
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Install 1" }));
    await waitFor(() => expect(execute).toHaveBeenCalledTimes(1));
    expect(execute).toHaveBeenCalledWith("op-One");
  });
});

describe("batch progress", () => {
  it("says which archive it is checking", async () => {
    let release: (value: OperationPreviewDto) => void = () => undefined;
    vi.spyOn(api, "inspectPackageForInstall")
      .mockResolvedValueOnce(preview("One"))
      .mockImplementationOnce(
        () =>
          new Promise<OperationPreviewDto>((resolve) => {
            release = resolve;
          }),
      );
    vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    render(
      <BatchInstall
        profileId="p1"
        paths={["/dl/One.zip", "/dl/Two.zip"]}
        onClose={vi.fn()}
      />,
    );
    expect(
      await screen.findByText("Checking 2 of 2: Two.zip"),
    ).toBeInTheDocument();
    release(preview("Two"));
    expect(
      await screen.findByRole("button", { name: "Install 2" }),
    ).toBeInTheDocument();
  });
});

describe("stopping a batch", () => {
  it("finishes the archive in progress and reports the rest as not started", async () => {
    vi.spyOn(api, "inspectPackageForInstall").mockImplementation(async (path) =>
      preview(path.includes("One") ? "One" : "Two"),
    );
    vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    let finish: () => void = () => undefined;
    const execute = vi.spyOn(api, "executeOperation").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = () => resolve({} as never);
        }),
    );
    render(
      <BatchInstall
        profileId="p1"
        paths={["/dl/One.zip", "/dl/Two.zip"]}
        onClose={vi.fn()}
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Install 2" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Stop after this one" }),
    );
    await waitFor(() => expect(execute).toHaveBeenCalledTimes(1));
    finish();
    expect(
      await screen.findByText(
        "1 of 2 installed; the rest are listed with why.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Not started: you stopped the batch/),
    ).toBeInTheDocument();
    expect(execute).toHaveBeenCalledTimes(1);
  });
});
