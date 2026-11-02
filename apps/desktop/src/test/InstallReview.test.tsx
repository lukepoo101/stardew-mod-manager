import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProfileModInstaller } from "@/features/mods/ProfileModInstaller";
import { api } from "@/shared/api/client";
import { ApiClientError } from "@/shared/api/errors";
import type {
  ModRequirementDto,
  OperationPreviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const requirement = (
  over: Partial<ModRequirementDto> = {},
): ModRequirementDto => ({
  unique_id: "Pat.ContentPatcher",
  name: null,
  installed_version: null,
  minimum_version: null,
  kind: "required",
  status: "missing",
  ...over,
});

const component = (
  id: string,
  requirements: ModRequirementDto[] = [],
): OperationPreviewDto["detected_components"][number] => ({
  unique_id: id,
  name: id.replace(".", " "),
  author: "A",
  version: "1.0.0",
  description: null,
  relative_root: id,
  requirements,
});

const preview = (
  over: Partial<OperationPreviewDto> = {},
): OperationPreviewDto => ({
  operation_id: "op-1",
  artifact_hash: "hash-1",
  original_filename: "mod.zip",
  byte_size: 1,
  detected_components: [component("Plain.Mod")],
  dependencies_satisfied: true,
  warnings: [],
  blockers: [],
  other_blockers: [],
  affected_profile_component_ids: [],
  expected_profile_revision: 1,
  files: [],
  replaces: [],
  ...over,
});

function renderInstaller() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ProfileModInstaller profileId="p1" />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return view;
}

async function open(p: OperationPreviewDto) {
  vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(p);
  vi.spyOn(api, "getProfileFreeze").mockResolvedValue(null);
  const view = renderInstaller();
  fireEvent.change(screen.getByLabelText("Mod archive path"), {
    target: { value: "/tmp/mod.zip" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Inspect" }));
  await screen.findByText("Review mod installation");
  return view;
}

describe("leaving an install review", () => {
  it("discards the prepared install when the review goes away", async () => {
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const view = await open(preview());
    // Ctrl+K or the sidebar navigates away and unmounts the page.
    view.unmount();
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("op-1"));
    expect(cancel).toHaveBeenCalledTimes(1);
  });

  it("discards it once when Cancel was pressed before leaving", async () => {
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const view = await open(preview());
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(screen.queryByText("Review mod installation")).toBeNull(),
    );
    view.unmount();
    expect(cancel).toHaveBeenCalledTimes(1);
  });

  it("never discards an install that was committed", async () => {
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    vi.spyOn(api, "executeOperation").mockResolvedValue({} as never);
    const view = await open(preview());
    fireEvent.click(screen.getByRole("button", { name: "Install mod" }));
    await screen.findByText("Installed 1 mod");
    view.unmount();
    expect(cancel).not.toHaveBeenCalled();
  });

  it("does not discard an install while its commit is running", async () => {
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    vi.spyOn(api, "executeOperation").mockReturnValue(
      new Promise(() => undefined),
    );
    const view = await open(preview());
    fireEvent.click(screen.getByRole("button", { name: "Install mod" }));
    view.unmount();
    expect(cancel).not.toHaveBeenCalled();
  });
});

describe("a missing requirement", () => {
  it("says which mods need what and finds a stored copy", async () => {
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    vi.spyOn(api, "findStoredMod").mockResolvedValue([
      {
        artifact_hash: "hash-cp",
        name: "Content Patcher",
        version: "2.0.0",
        original_filename: "CP.zip",
        meets_minimum: true,
      },
    ]);
    const installStored = vi
      .spyOn(api, "installStoredPackage")
      .mockResolvedValue();
    const blocked = preview({
      detected_components: [
        component("Two.A", [requirement()]),
        component("Two.B", [requirement()]),
      ],
      dependencies_satisfied: false,
      blockers: ["Two A needs Pat.ContentPatcher.", "Two B needs it too."],
    });
    await open(blocked);
    expect(
      screen.getByText("This cannot be installed yet"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Two A and Two B need the mod with UniqueID Pat.ContentPatcher, which is not installed.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Install 2 mods" }),
    ).toBeDisabled();

    // The stored copy is offered without another click.
    expect(
      await screen.findByText(/Content Patcher 2.0.0 from CP.zip/),
    ).toBeInTheDocument();
    const inspect = vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(
      preview({
        operation_id: "op-2",
        detected_components: [component("Two.A"), component("Two.B")],
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Install it" }));
    // The review lets go of the profile, the requirement goes in, and the
    // archive is checked again.
    await waitFor(() =>
      expect(installStored).toHaveBeenCalledWith("p1", "hash-cp", true),
    );
    expect(cancel).toHaveBeenCalledWith("op-1");
    expect(cancel.mock.invocationCallOrder[0]).toBeLessThan(
      installStored.mock.invocationCallOrder[0],
    );
    await waitFor(() =>
      expect(inspect).toHaveBeenCalledWith("/tmp/mod.zip", "p1"),
    );
    expect(
      await screen.findByRole("button", { name: "Install 2 mods" }),
    ).toBeEnabled();
  });

  it("gives the UniqueID to look for when nothing is stored", async () => {
    vi.spyOn(api, "findStoredMod").mockResolvedValue([]);
    await open(
      preview({
        detected_components: [
          component("Chain.Top", [
            requirement({ unique_id: "Chain.Middle", minimum_version: "1.0" }),
          ]),
        ],
        dependencies_satisfied: false,
        blockers: ["Chain Top needs Chain.Middle."],
      }),
    );
    expect(
      await screen.findByText(/No stored copy. Look for/),
    ).toHaveTextContent("Chain.Middle 1.0 or newer");
  });

  it("names a requirement that is too old with both versions", async () => {
    vi.spyOn(api, "findStoredMod").mockResolvedValue([]);
    await open(
      preview({
        detected_components: [
          component("Picky.Mod", [
            requirement({
              unique_id: "spacechase0.SpaceCore",
              name: "SpaceCore",
              installed_version: "1.25.0",
              minimum_version: "9.0.0",
              status: "too_old",
            }),
          ]),
        ],
        dependencies_satisfied: false,
        blockers: ["Picky needs a newer SpaceCore."],
      }),
    );
    expect(
      screen.getByText(
        "Picky Mod needs SpaceCore 9.0.0 or newer; 1.25.0 is installed.",
      ),
    ).toBeInTheDocument();
  });

  it("shows package blockers that no requirement explains", async () => {
    await open(
      preview({
        dependencies_satisfied: false,
        blockers: ["Big Map needs SMAPI 9.0.0 or newer."],
        other_blockers: ["Big Map needs SMAPI 9.0.0 or newer."],
      }),
    );
    expect(
      screen.getByText("Big Map needs SMAPI 9.0.0 or newer."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install mod" })).toBeDisabled();
  });
});

describe("replacing a mod others need", () => {
  it("names the mods that will stop loading", async () => {
    await open(
      preview({
        blockers: ["A mod with this UniqueID is already installed"],
        other_blockers: ["A mod with this UniqueID is already installed"],
        replaces: [
          {
            profile_component_id: "c",
            unique_id: "Chain.Core",
            name: "Chain Core",
            installed_version: "3.0.0",
            incoming_version: "1.5.0",
            direction: "downgrade",
            stops_loading: [
              "Chain Middle needs Chain Core 2.0.0 or newer, so it will not load with 1.5.0.",
            ],
          },
        ],
      }),
    );
    expect(
      screen.getByText("These enabled mods will stop loading:"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Chain Middle needs Chain Core 2.0.0 or newer/),
    ).toBeInTheDocument();
    expect(screen.queryByText("This cannot be installed yet")).toBeNull();
  });
});

describe("a profile frozen while the review is open", () => {
  it("keeps the install button off after the refusal", async () => {
    vi.spyOn(api, "executeOperation").mockRejectedValue(
      new ApiClientError({
        code: "PROFILE_FROZEN",
        category: "validation",
        summary: "This profile is frozen. Unfreeze it on the Profiles page.",
        technical_details: null,
        recoverability: "none",
        operation_id: null,
      } as never),
    );
    await open(preview());
    const install = screen.getByRole("button", { name: "Install mod" });
    fireEvent.click(install);
    expect(
      await screen.findByText(/This profile is frozen/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install mod" })).toBeDisabled();
  });

  it("is off from the start when the profile is already frozen", async () => {
    vi.spyOn(api, "inspectPackageForInstall").mockResolvedValue(preview());
    vi.spyOn(api, "getProfileFreeze").mockResolvedValue({
      reason: "",
    } as never);
    renderInstaller();
    fireEvent.change(screen.getByLabelText("Mod archive path"), {
      target: { value: "/tmp/mod.zip" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Inspect" }));
    expect(
      await screen.findByText(/This profile is frozen, so nothing can be/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install mod" })).toBeDisabled();
  });
});
