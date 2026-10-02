import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CollectionCard } from "@/features/profiles/CollectionCard";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import {
  buildCollectionRecipe,
  newDraft,
  readDraft,
} from "@/shared/recipe/collection";
import { parseRecipe } from "@/shared/recipe/recipe";
import * as actions from "@/shared/support/actions";

afterEach(() => vi.restoreAllMocks());

const overview = {
  profile: { id: "p1", name: "Cozy", revision: 1 },
  game: { storefront: "steam" },
  smapi_status: { observed_version: "4.1.10" },
} as unknown as ProfileOverviewDto;

const mod = (id: string, version = "1.0") =>
  ({
    profile_component_id: `c-${id}`,
    unique_id: id,
    name: id,
    author: "x",
    version,
    enabled: true,
    artifact_hash: "",
  }) as ModListItemDto;

describe("collection recipes from a draft", () => {
  it("applies the curator's choices and names the revision", () => {
    const draft = {
      ...newDraft("Cozy Valley", "c1"),
      author: "Me",
      groups: [
        { name: "Portraits", description: "Pick one", choose: "one" as const },
        { name: "Unused", description: "", choose: "any" as const },
      ],
      mods: {
        "a.mod": { newerOk: true },
        "b.mod": { group: "Portraits" },
        "c.mod": {
          manualUrl: "https://forums.example.com/1",
          manualInstructions: "zip",
        },
      },
    };
    const recipe = buildCollectionRecipe(
      overview,
      [mod("A.Mod"), mod("B.Mod"), mod("C.Mod")],
      draft,
      4,
      "2026-10-02T00:00:00Z",
    );
    expect(recipe.collection).toMatchObject({
      id: "c1",
      revision: 4,
      author: "Me",
    });
    const [a, b, c] = recipe.components;
    expect(a.version_rule).toBe("at_least");
    expect(b.group).toBe("Portraits");
    expect(b.optional).toBe(true);
    expect(c.manual?.url).toMatch(/forums/);
    expect(recipe.groups?.map((g) => g.name)).toEqual(["Portraits"]);
    // What it builds is a valid recipe.
    expect(parseRecipe(JSON.stringify(recipe)).ok).toBe(true);
  });

  it("falls back to a fresh draft when the saved one is unreadable", () => {
    const fresh = newDraft("Cozy", "new");
    expect(readDraft("{oops", fresh)).toBe(fresh);
    expect(
      readDraft(JSON.stringify({ id: "c1", name: "Kept" }), fresh).name,
    ).toBe("Kept");
  });
});

describe("publishing a collection", () => {
  it("publishes the next revision and shows what changed since the last", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([mod("A.Mod", "2.0")]);
    vi.spyOn(api, "getCollectionDraft").mockResolvedValue(
      JSON.stringify({ ...newDraft("Cozy Valley", "c1"), author: "Me" }),
    );
    const previous = buildCollectionRecipe(
      overview,
      [mod("A.Mod", "1.0")],
      newDraft("Cozy Valley", "c1"),
      1,
      "2026-09-01T00:00:00Z",
    );
    vi.spyOn(api, "listCollectionRevisions").mockResolvedValue([
      {
        collection_id: "c1",
        revision: 1,
        published_at: "2026-09-01T00:00:00Z",
        recipe_json: JSON.stringify(previous),
      },
    ]);
    vi.spyOn(api, "saveCollectionDraft").mockResolvedValue();
    const publish = vi
      .spyOn(api, "publishCollectionRevision")
      .mockResolvedValue({} as never);
    const download = vi
      .spyOn(actions, "downloadText")
      .mockImplementation(() => {});
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <CollectionCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(/Changes since revision 1/),
    ).toBeInTheDocument();
    // Changes without notes are a warning: publishing waits for it to be read.
    expect(
      screen.getByRole("button", { name: "Publish revision 2" }),
    ).toBeDisabled();
    expect(screen.getByText(/no notes saying why/)).toBeInTheDocument();
    fireEvent.click(
      screen.getByLabelText(
        /I have read the warnings and want to publish anyway/,
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Publish revision 2" }));
    await waitFor(() => expect(publish).toHaveBeenCalled());
    const published = JSON.parse(publish.mock.calls[0][0]);
    expect(published.collection.revision).toBe(2);
    expect(published.components[0].version).toBe("2.0");
    expect(download.mock.calls[0][0]).toBe("Cozy-Valley-r2.json");
  });
});

describe("testing a collection in a clean profile", () => {
  it("makes an empty profile that follows the latest revision", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      ...overview,
      game: { id: "g1", storefront: "steam" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([mod("A.Mod")]);
    vi.spyOn(api, "getCollectionDraft").mockResolvedValue(
      JSON.stringify(newDraft("Cozy Valley", "c1")),
    );
    vi.spyOn(api, "listCollectionRevisions").mockResolvedValue([
      {
        collection_id: "c1",
        revision: 2,
        published_at: "2026-09-01T00:00:00Z",
        recipe_json: '{"recipe":2}',
      },
    ]);
    const create = vi
      .spyOn(api, "createProfile")
      .mockResolvedValue({ id: "t1", name: "Cozy Valley r2 test" } as never);
    const attach = vi
      .spyOn(api, "attachReferenceRecipe")
      .mockResolvedValue({} as never);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <CollectionCard />
      </QueryClientProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Try the latest revision in a clean profile",
      }),
    );
    await waitFor(() =>
      expect(attach).toHaveBeenCalledWith("t1", '{"recipe":2}'),
    );
    expect(create).toHaveBeenCalledWith("Cozy Valley r2 test", "g1");
    expect(
      await screen.findByText(/an empty profile following revision 2/),
    ).toBeInTheDocument();
  });
});
