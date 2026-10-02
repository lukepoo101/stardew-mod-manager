import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BatchInstall } from "@/features/mods/BatchInstall";
import { api } from "@/shared/api/client";
import type { OperationPreviewDto } from "@/shared/api/generated";
import { classifyBatch } from "@/shared/mods/batch";

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
    },
  ],
  dependencies_satisfied: true,
  warnings: [],
  blockers: [],
  affected_profile_component_ids: [],
  expected_profile_revision: null,
  files: [],
  replaces: [],
  ...over,
});

describe("classifying a batch", () => {
  it("says what happens to each archive before anything changes", () => {
    const entries = classifyBatch([
      { path: "/dl/Lib.zip", preview: preview("Z.Lib") },
      {
        path: "/dl/Needy.zip",
        preview: preview("A.Needy", {
          blockers: ["Required dependency 'Z.Lib' is missing"],
        }),
      },
      {
        path: "/dl/Orphan.zip",
        preview: preview("A.Orphan", {
          blockers: ["Required dependency 'Gone.Mod' is missing"],
        }),
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
          ? preview("A.Needy", {
              blockers:
                inspect.mock.calls.length > 2
                  ? []
                  : ["Required dependency 'Z.Lib' is missing"],
            })
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
      await screen.findByText(/Needs a mod from another archive here/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Install 2" }));
    expect(await screen.findByText("2 of 2 installed.")).toBeInTheDocument();
    // Lib before Needy, each freshly prepared.
    expect(execute.mock.calls.map((c) => c[0])).toEqual([
      "op-Z.Lib",
      "op-A.Needy",
    ]);
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
    expect(await screen.findByText("1 of 2 installed.")).toBeInTheDocument();
    expect(
      screen.getByText(/Not started: you stopped the batch/),
    ).toBeInTheDocument();
    expect(execute).toHaveBeenCalledTimes(1);
  });
});
