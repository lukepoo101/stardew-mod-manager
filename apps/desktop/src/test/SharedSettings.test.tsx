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
import { diffRecipes } from "@/shared/recipe/curator";
import { newDraft } from "@/shared/recipe/collection";
import { parseRecipe, type ProfileRecipe } from "@/shared/recipe/recipe";
import { settingsDifferences } from "@/shared/recipe/settings";
import * as actions from "@/shared/support/actions";

afterEach(() => vi.restoreAllMocks());

const SHARED = { path: "config.json", sha256: "c".repeat(64), content: "{}" };

const recipeWith = (settings: unknown[] | undefined) =>
  ({
    schema: "stardew-mod-manager.profile-recipe",
    schema_version: 1,
    generated_at: "",
    profile_name: "Cozy",
    game: { storefront: "steam", smapi_version: null },
    components: [
      {
        unique_id: "A.Mod",
        name: "A",
        author: "a",
        version: "1.0",
        enabled: true,
        artifact_hash: "a".repeat(64),
        optional: false,
        ...(settings ? { settings } : {}),
      },
    ],
  }) as ProfileRecipe;

const mod = {
  profile_component_id: "c-a",
  unique_id: "A.Mod",
  name: "A",
  author: "a",
  version: "1.0",
  enabled: true,
  artifact_hash: "a".repeat(64),
} as ModListItemDto;

describe("settings in recipes", () => {
  it("parses shared settings and rejects unsafe paths", () => {
    expect(parseRecipe(JSON.stringify(recipeWith([SHARED]))).ok).toBe(true);
    const bad = parseRecipe(
      JSON.stringify(recipeWith([{ ...SHARED, path: "../config.json" }])),
    );
    expect(bad.ok).toBe(false);
    const other = parseRecipe(
      JSON.stringify(recipeWith([{ ...SHARED, path: "data.json" }])),
    );
    expect(other.ok).toBe(false);
  });

  it("finds differing settings by checksum, only for installed mods", () => {
    const recipe = recipeWith([SHARED]);
    const same = [
      { unique_id: "a.mod", path: "config.json", sha256: SHARED.sha256 },
    ];
    const other = [
      { unique_id: "a.mod", path: "config.json", sha256: "d".repeat(64) },
    ];
    expect(settingsDifferences(recipe, same, new Set(["a.mod"]))).toEqual([]);
    const diff = settingsDifferences(recipe, other, new Set(["a.mod"]));
    expect(diff[0].files).toEqual(["config.json"]);
    expect(settingsDifferences(recipe, other, new Set())).toEqual([]);
    // The key changes when either side's settings change.
    const otherAgain = [{ ...other[0], sha256: "e".repeat(64) }];
    expect(
      settingsDifferences(recipe, otherAgain, new Set(["a.mod"]))[0].key,
    ).not.toBe(diff[0].key);
  });

  it("reports shared-settings changes between revisions", () => {
    const log = diffRecipes(recipeWith(undefined), recipeWith([SHARED]));
    expect(log.termsChanged[0].changes).toContain(
      "its settings are now shared",
    );
  });
});

describe("recipients and shared settings", () => {
  it("offers the shared settings, applied only after confirming", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Mine" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([mod]);
    vi.spyOn(api, "getReferenceRecipe").mockResolvedValue({
      recipe_json: JSON.stringify(recipeWith([SHARED])),
      attached_at: "2026-10-01T00:00:00Z",
      accepted: [],
      accepted_notes: {},
    });
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    vi.spyOn(api, "settingsHashes").mockResolvedValue([
      { unique_id: "a.mod", path: "config.json", sha256: "d".repeat(64) },
    ]);
    const apply = vi.spyOn(api, "applySharedSettings").mockResolvedValue("b1");
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ReferenceCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(/config\.json differ from the shared settings/),
    ).toBeInTheDocument();
    expect(screen.getByText("1 difference(s)")).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Use the shared settings" }),
    );
    await waitFor(() =>
      expect(apply).toHaveBeenCalledWith("p1", "A.Mod", [SHARED]),
    );
  });
});

describe("putting every difference right", () => {
  it("includes shared settings that differ", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Mine" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([mod]);
    vi.spyOn(api, "getReferenceRecipe").mockResolvedValue({
      recipe_json: JSON.stringify(recipeWith([SHARED])),
      attached_at: "2026-10-01T00:00:00Z",
      accepted: [],
      accepted_notes: {},
    });
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    vi.spyOn(api, "settingsHashes").mockResolvedValue([
      { unique_id: "a.mod", path: "config.json", sha256: "d".repeat(64) },
    ]);
    vi.spyOn(api, "createRestorePoint").mockResolvedValue({} as never);
    const apply = vi.spyOn(api, "applySharedSettings").mockResolvedValue("b1");
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ReferenceCard />
      </QueryClientProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Put every difference right...",
      }),
    );
    await waitFor(() =>
      expect(apply).toHaveBeenCalledWith("p1", "A.Mod", [SHARED]),
    );
    expect(confirm.mock.calls[0][0]).toMatch(/use the shared settings/);
  });
});

describe("curators sharing settings", () => {
  it("includes a mod's settings only when chosen, with privacy warnings", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Cozy", revision: 1 },
      game: { storefront: "steam" },
      smapi_status: { observed_version: "4.1.10" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([mod]);
    vi.spyOn(api, "getCollectionDraft").mockResolvedValue(
      JSON.stringify({
        ...newDraft("Cozy", "c1"),
        author: "Me",
        mods: { "a.mod": { includeSettings: true } },
      }),
    );
    vi.spyOn(api, "listCollectionRevisions").mockResolvedValue([]);
    vi.spyOn(api, "listShareableSettings").mockResolvedValue([
      {
        unique_id: "A.Mod",
        name: "A",
        files: ["config.json"],
        warnings: ["a key that looks like a password at $.Password"],
      },
    ]);
    const read = vi
      .spyOn(api, "readSharedSettings")
      .mockResolvedValue([
        { unique_id: "A.Mod", files: [SHARED], skipped: [] },
      ]);
    vi.spyOn(api, "saveCollectionDraft").mockResolvedValue();
    const publish = vi
      .spyOn(api, "publishCollectionRevision")
      .mockResolvedValue({} as never);
    vi.spyOn(actions, "downloadText").mockImplementation(() => {});
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <CollectionCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(/settings may include private details/),
    ).toBeInTheDocument();
    expect(read).toHaveBeenCalledWith("p1", ["a.mod"]);
    fireEvent.click(
      screen.getByLabelText(
        /I have read the warnings and want to publish anyway/,
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Publish revision 1" }));
    await waitFor(() => expect(publish).toHaveBeenCalled());
    const published = JSON.parse(publish.mock.calls[0][0]);
    expect(published.components[0].settings).toEqual([SHARED]);
  });
});

describe("frozen shared setups", () => {
  it("keeps the frozen marker when parsing and shows it to recipients", async () => {
    const recipe = {
      ...recipeWith(undefined),
      frozen: { frozen_at: "2026-10-01T18:00:00Z", reason: "Saturday co-op" },
    };
    const parsed = parseRecipe(JSON.stringify(recipe));
    expect(parsed.ok && parsed.recipe.frozen?.reason).toBe("Saturday co-op");
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Mine" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([mod]);
    vi.spyOn(api, "getReferenceRecipe").mockResolvedValue({
      recipe_json: JSON.stringify(recipe),
      attached_at: "2026-10-01T00:00:00Z",
      accepted: [],
      accepted_notes: {},
    });
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ReferenceCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(/These are the versions agreed for the group/),
    ).toBeInTheDocument();
    expect(screen.getByText(/Saturday co-op/)).toBeInTheDocument();
  });
});
