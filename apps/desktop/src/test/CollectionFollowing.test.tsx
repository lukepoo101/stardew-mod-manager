import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CollectionCard } from "@/features/profiles/CollectionCard";
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
  artifact_hash: (id === "A.Mod" ? "a" : "f").repeat(64),
  optional: false,
  ...extra,
});

const collection = (revision: number, components: unknown[]) =>
  JSON.stringify({
    schema: "stardew-mod-manager.profile-recipe",
    schema_version: 1,
    generated_at: "2026-10-01T00:00:00Z",
    profile_name: "Cozy",
    game: { storefront: "steam", smapi_version: null },
    components,
    collection: {
      id: "c1",
      name: "Cozy",
      author: "Ann",
      revision,
      notes: "Big update",
    },
  });

const installed = (version: string) =>
  [
    {
      profile_component_id: "c-a",
      unique_id: "A.Mod",
      name: "A.Mod",
      author: "a",
      version,
      enabled: true,
      artifact_hash: "b".repeat(64),
    },
  ] as ModListItemDto[];

function renderReference(recipeJson: string, mods: ModListItemDto[]) {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p1", name: "Mine" },
  } as unknown as ProfileOverviewDto);
  vi.spyOn(api, "listProfileMods").mockResolvedValue(mods);
  vi.spyOn(api, "getReferenceRecipe").mockResolvedValue({
    recipe_json: recipeJson,
    attached_at: "2026-10-01T00:00:00Z",
    accepted: [],
  });
  vi.spyOn(api, "storedPackages").mockResolvedValue(["a".repeat(64)]);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ReferenceCard />
    </QueryClientProvider>,
  );
}

describe("following a collection", () => {
  it("names the collection and revision, and the manual source of a missing mod", async () => {
    renderReference(
      collection(2, [
        component("A.Mod", "1.0"),
        component("F.Mod", "1.0", {
          manual: {
            url: "https://forums.example.com/1",
            instructions: "the zip",
          },
        }),
      ]),
      installed("1.0"),
    );
    expect(
      await screen.findByText(/Collection "Cozy" revision 2 by Ann/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /by hand from https:\/\/forums.example.com\/1 \(the zip\)/,
      ),
    ).toBeInTheDocument();
  });

  it("shows what a newer revision changes before following it", async () => {
    renderReference(
      collection(1, [component("A.Mod", "1.0")]),
      installed("1.0"),
    );
    const attach = vi
      .spyOn(api, "attachReferenceRecipe")
      .mockResolvedValue({} as never);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    await screen.findByText(/Collection "Cozy" revision 1/);
    const file = new File(
      [collection(2, [component("A.Mod", "2.0")])],
      "cozy-r2.json",
      { type: "application/json" },
    );
    fireEvent.change(screen.getByLabelText("Group recipe file"), {
      target: { files: [file] },
    });
    await waitFor(() => expect(attach).toHaveBeenCalled());
    expect(confirm.mock.calls[0][0]).toMatch(/Update to revision 2 of "Cozy"/);
    expect(confirm.mock.calls[0][0]).toMatch(/A.Mod/);
  });

  it("puts every fixable difference right after one review, saving a restore point first", async () => {
    renderReference(
      collection(1, [component("A.Mod", "1.0")]),
      installed("2.0"),
    );
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const point = vi
      .spyOn(api, "createRestorePoint")
      .mockResolvedValue({} as never);
    const replace = vi
      .spyOn(api, "replaceModVersion")
      .mockResolvedValue({} as never);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Put every difference right...",
      }),
    );
    await waitFor(() =>
      expect(replace).toHaveBeenCalledWith("p1", "a".repeat(64)),
    );
    expect(point.mock.invocationCallOrder[0]).toBeLessThan(
      replace.mock.invocationCallOrder[0],
    );
  });
});

describe("forking a followed collection", () => {
  it("starts a new collection based on it and shows what differs", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Mine" },
      game: { storefront: "steam" },
      smapi_status: { observed_version: null },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue(installed("2.0"));
    vi.spyOn(api, "getCollectionDraft").mockResolvedValue(null);
    vi.spyOn(api, "listCollectionRevisions").mockResolvedValue([]);
    vi.spyOn(api, "getReferenceRecipe").mockResolvedValue({
      recipe_json: collection(3, [component("A.Mod", "1.0")]),
      attached_at: "2026-10-01T00:00:00Z",
      accepted: [],
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <CollectionCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(/Based on "Cozy" revision 3/),
    ).toBeInTheDocument();
    expect(screen.getByDisplayValue("Cozy (my version)")).toBeInTheDocument();
    expect(
      screen.getByText(/Changes from "Cozy" revision 3/),
    ).toBeInTheDocument();
  });
});

describe("client-only mods in a collection", () => {
  it("lists them apart and does not count them as differences", async () => {
    renderReference(
      collection(1, [
        component("A.Mod", "1.0"),
        component("F.Mod", "1.0", { client_only: true }),
      ]),
      installed("1.0"),
    );
    expect(
      await screen.findByText(/Client-only differences, which need not match/),
    ).toBeInTheDocument();
    // Only A.Mod's package difference counts; the client-only F.Mod does not.
    expect(screen.getByText("1 difference(s)")).toBeInTheDocument();
  });
});
