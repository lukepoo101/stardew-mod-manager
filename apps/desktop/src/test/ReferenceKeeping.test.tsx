import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ReferenceCard } from "@/features/profiles/ReferenceCard";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const component = (id: string, version: string, extra = {}) => ({
  unique_id: id,
  name: id,
  author: "a",
  version,
  enabled: true,
  artifact_hash: id.charAt(0).toLowerCase().repeat(64),
  optional: false,
  ...extra,
});

const recipe = (components: unknown[], revision?: number) =>
  JSON.stringify({
    schema: "stardew-mod-manager.profile-recipe",
    schema_version: 1,
    generated_at: "2026-10-01T00:00:00Z",
    profile_name: "Group",
    game: { storefront: "steam", smapi_version: null },
    components,
    ...(revision
      ? {
          collection: {
            id: "c1",
            name: "Cozy",
            author: "Ann",
            revision,
            notes: "",
          },
        }
      : {}),
  });

const mod = (id: string, version: string, extra = {}) =>
  ({
    profile_component_id: `c-${id}`,
    unique_id: id,
    name: id,
    author: "a",
    version,
    enabled: true,
    artifact_hash: id.charAt(0).toLowerCase().repeat(64),
    ...extra,
  }) as ModListItemDto;

function renderReference(
  recipeJson: string,
  mods: ModListItemDto[],
  accepted: string[] = [],
  stored: string[] = [],
  operatingSystem = "linux",
) {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p1", name: "Mine" },
    game: { operating_system: operatingSystem },
  } as unknown as ProfileOverviewDto);
  vi.spyOn(api, "listProfileMods").mockResolvedValue(mods);
  vi.spyOn(api, "getReferenceRecipe").mockResolvedValue({
    recipe_json: recipeJson,
    attached_at: "2026-10-01T00:00:00Z",
    accepted,
    accepted_notes: Object.fromEntries(accepted.map((k) => [k, "my reason"])),
  });
  const storedPackages = vi
    .spyOn(api, "storedPackages")
    .mockResolvedValue(stored);
  const client = new QueryClient();
  render(
    <QueryClientProvider client={client}>
      <ReferenceCard />
    </QueryClientProvider>,
  );
  return { client, storedPackages };
}

const upload = (text: string) =>
  fireEvent.change(screen.getByLabelText("Group recipe file"), {
    target: {
      files: [new File([text], "group.json", { type: "application/json" })],
    },
  });

describe("replacing a group reference", () => {
  it("keeps the acceptances whose difference is unchanged and shows the rest", async () => {
    // Disabled here on purpose; Lookup is accepted at 1.49 vs 1.48.
    const accepted = [
      "enabled:C.Cheats:1.0:1.0",
      "version:L.Lookup:1.49.0:1.48.0",
    ];
    renderReference(
      recipe([component("C.Cheats", "1.0"), component("L.Lookup", "1.49.0")]),
      [mod("C.Cheats", "1.0", { enabled: false }), mod("L.Lookup", "1.48.0")],
      accepted,
    );
    const attach = vi
      .spyOn(api, "attachReferenceRecipe")
      .mockResolvedValue({} as never);
    await screen.findByText(/Accepted for this group \(2\)/);
    // The group moves Lookup to 1.50.0 and leaves Cheats as it was.
    upload(
      recipe([component("C.Cheats", "1.0"), component("L.Lookup", "1.50.0")]),
    );
    await waitFor(() => expect(attach).toHaveBeenCalled());
    const keep = attach.mock.calls[0][2] ?? [];
    expect(keep).toContain("enabled:C.Cheats:1.0:1.0");
    expect(keep).not.toContain("version:L.Lookup:1.49.0:1.48.0");
    expect(keep).toContain("version:L.Lookup:1.50.0:1.48.0");
  });
});

describe("taking a new collection revision", () => {
  it("keeps your accepted changes, skips unstored packages and other systems, and accepts what it turned off", async () => {
    const before = recipe(
      [
        component("T.Tractor", "1.0"),
        component("M.Music", "0.5"),
        component("U.Ui", "2.3"),
      ],
      1,
    );
    renderReference(
      before,
      [
        mod("T.Tractor", "1.0", { enabled: false }),
        mod("M.Music", "0.5"),
        mod("U.Ui", "2.3"),
        mod("P.Hats", "1.0"),
      ],
      ["enabled:T.Tractor:1.0:1.0", "extra:P.Hats::1.0"],
      // Only the music mod's package is stored: U.Ui 2.4 and W.Shader are not.
      [],
    );
    const attach = vi
      .spyOn(api, "attachReferenceRecipe")
      .mockResolvedValue({} as never);
    vi.spyOn(api, "createRestorePoint").mockResolvedValue({} as never);
    const toggle = vi
      .spyOn(api, "setModsEnabled")
      .mockResolvedValue({ changed: [], failed: [] } as never);
    const install = vi.spyOn(api, "installStoredPackage");
    const begin = vi
      .spyOn(api, "beginChangeSet")
      .mockResolvedValue("j1" as never);
    vi.spyOn(api, "changeSetPartDone").mockResolvedValue(undefined as never);
    const finish = vi
      .spyOn(api, "finishChangeSet")
      .mockResolvedValue(undefined as never);
    const accept = vi
      .spyOn(api, "setReferenceDifferenceAccepted")
      .mockResolvedValue({} as never);
    await screen.findByText(/Collection "Cozy" revision 1/);
    // Revision 2: U.Ui updated (package not stored), M.Music removed, and a
    // Windows-only shader added.
    upload(
      recipe(
        [
          component("T.Tractor", "1.0"),
          component("U.Ui", "2.4", { artifact_hash: "9".repeat(64) }),
          component("W.Shader", "1.0", { platforms: ["windows"] }),
        ],
        2,
      ),
    );
    expect(
      await screen.findByText(/Not for this computer \(linux\)/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Update" }));
    await waitFor(() => expect(finish).toHaveBeenCalled());

    // Your own accepted changes are named as still applying.
    const keep = attach.mock.calls[0][2] ?? [];
    expect(keep).toEqual(
      expect.arrayContaining([
        "enabled:T.Tractor:1.0:1.0",
        "extra:P.Hats::1.0",
      ]),
    );
    // Only what can be done here is in the journaled change, so nothing
    // expected is logged as failed.
    expect(begin.mock.calls[0][2]).toEqual(["M.Music: removed"]);
    expect(finish).toHaveBeenCalledWith("j1", []);
    expect(toggle).toHaveBeenCalledWith(["c-M.Music"], false);
    expect(install).not.toHaveBeenCalled();
    // The mod it turned off is accepted, with why, rather than an extra.
    await waitFor(() =>
      expect(accept).toHaveBeenCalledWith(
        "p1",
        "extra:M.Music::0.5",
        true,
        expect.stringContaining("Removed in revision 2"),
      ),
    );
    expect(
      await screen.findByText(/Left as differences until their packages/),
    ).toBeInTheDocument();
  });
});

describe("group reference rows", () => {
  it("shows the curator's reason for a required mod", async () => {
    renderReference(
      recipe([
        component("C.Patcher", "2.0", {
          note: "Framework every content pack here needs",
        }),
      ]),
      [],
    );
    expect(
      await screen.findByText(/Framework every content pack here needs/),
    ).toBeInTheDocument();
  });

  it("offers the fix as soon as the package is stored, without reopening", async () => {
    const { client, storedPackages } = renderReference(
      recipe([component("D.Elevator", "1.6.0")]),
      [],
    );
    expect(
      await screen.findByText(/then choose the downloaded file here/),
    ).toBeInTheDocument();
    // A bundle imported into another profile stores the package; the
    // backend's change event invalidates the queries.
    storedPackages.mockResolvedValue(["d".repeat(64)]);
    await client.invalidateQueries();
    expect(
      await screen.findByRole("button", { name: "Install" }),
    ).toBeInTheDocument();
  });
});
