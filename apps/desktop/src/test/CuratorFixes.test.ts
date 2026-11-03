import { describe, expect, it } from "bun:test";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import {
  buildCollectionRecipe,
  forkDraft,
  newDraft,
} from "@/shared/recipe/collection";
import { checkCollection } from "@/shared/recipe/collectionChecks";
import {
  checkRecipe,
  diffRecipes,
  renderChangelog,
} from "@/shared/recipe/curator";
import {
  compareWithRecipe,
  type ProfileRecipe,
  type RecipeComponent,
} from "@/shared/recipe/recipe";

const overview = {
  profile: { id: "p1", name: "Cozy" },
  game: { storefront: "steam" },
  smapi_status: { observed_version: "4.1.10" },
} as unknown as ProfileOverviewDto;
const mod = (id: string, extra: Partial<ModListItemDto> = {}) =>
  ({
    profile_component_id: `c-${id}`,
    unique_id: id,
    name: id,
    author: "x",
    version: "1.0",
    enabled: true,
    artifact_hash: "a".repeat(64),
    ...extra,
  }) as ModListItemDto;
const component = (
  id: string,
  extra: Partial<RecipeComponent> = {},
): RecipeComponent => ({
  unique_id: id,
  name: id,
  author: "x",
  version: "1.0",
  enabled: true,
  artifact_hash: "a".repeat(64),
  optional: false,
  ...extra,
});
const recipe = (components: RecipeComponent[]): ProfileRecipe => ({
  schema: "stardew-mod-manager.profile-recipe",
  schema_version: 1,
  generated_at: "",
  profile_name: "Main",
  game: { storefront: "steam", smapi_version: "4.1.10" },
  components,
});

describe("the changelog", () => {
  it("lists a mod that was updated and turned off under both", () => {
    const log = diffRecipes(
      recipe([component("Bouhm.NPCMapLocations", { version: "3.2.1" })]),
      recipe([
        component("Bouhm.NPCMapLocations", {
          version: "3.3.0",
          enabled: false,
        }),
      ]),
    );
    expect(log.upgraded).toHaveLength(1);
    expect(log.enabledChanged).toHaveLength(1);
    const text = renderChangelog(recipe([]), recipe([]), log);
    expect(text).toMatch(/3\.2\.1 to 3\.3\.0/);
    expect(text).toMatch(/now disabled/);
  });

  it("matches mods by UniqueID whatever its case", () => {
    const log = diffRecipes(
      recipe([component("Tester.Mod")]),
      recipe([component("tester.mod")]),
    );
    expect(log.added).toHaveLength(0);
    expect(log.removed).toHaveLength(0);
    expect(log.unchanged).toBe(1);
  });
});

describe("reproducibility", () => {
  it("does not count a mod that accepts newer versions as pinned", () => {
    const report = checkRecipe(
      recipe([
        component("A"),
        component("B", { version_rule: "at_least" }),
        component("C"),
        component("D"),
      ]),
    );
    expect(report.reproducibility).toBe(75);
  });

  it("counts each mod once for what a recipient meets", () => {
    const draft = {
      ...newDraft("Cozy", "c1"),
      mods: {
        "m.hand": { manualUrl: "https://example.com/m" },
        "n.newer": { newerOk: true },
      },
    };
    const built = buildCollectionRecipe(
      overview,
      [mod("A.Exact"), mod("M.Hand"), mod("N.Newer")],
      draft,
      1,
      "now",
    );
    const { recipient } = checkCollection(built, draft);
    expect(recipient).toMatchObject({
      exactFiles: 1,
      manual: 1,
      newerAllowed: 1,
    });
  });

  it("says a newer version is fine when a mod is missing", () => {
    const comparison = compareWithRecipe(
      [],
      recipe([component("L.Lookup", { version_rule: "at_least" })]),
    );
    expect(comparison.differences[0].detail).toMatch(/1\.0 or newer/);
  });
});

describe("pre-share checks", () => {
  it("treats a manifest with no Author as missing an author", () => {
    const report = checkRecipe(
      recipe([component("Tester.NoAuthor", { author: "Unknown" })]),
    );
    expect(report.checks.find((c) => c.id === "attribution")?.status).toBe(
      "warn",
    );
  });

  it("does not pass the enabled check for an empty list", () => {
    const report = checkRecipe(recipe([]));
    expect(report.checks.find((c) => c.id === "enabled")).toBeUndefined();
    expect(report.checks.find((c) => c.id === "empty")?.status).toBe("fail");
  });
});

describe("manual download links", () => {
  it("blocks publishing when a download page is not a web address", () => {
    const draft = {
      ...newDraft("Cozy", "c1"),
      mods: {
        "k.fish": {
          manualUrl: "ask Kedi on Discord",
          manualInstructions: "DM Kedi for the beta zip",
        },
        "w.www": { manualUrl: "www.example.com/mod" },
      },
    };
    const built = buildCollectionRecipe(
      overview,
      [mod("K.Fish"), mod("W.Www")],
      draft,
      1,
      "now",
    );
    const { checks } = checkCollection(built, draft);
    expect(
      checks.find(
        (c) => c.level === "error" && c.message.includes("ask Kedi on Discord"),
      ),
    ).toBeDefined();
    // A plain www address is made a web address rather than dropped.
    expect(
      built.components.find((c) => c.unique_id === "W.Www")?.manual?.url,
    ).toBe("https://www.example.com/mod");
  });
});

describe("forking a collection", () => {
  it("carries the original's curation over and credits its author", () => {
    const parent: ProfileRecipe = {
      ...recipe([
        component("A.Portrait", { optional: true, group: "Portraits" }),
        component("C.Patcher", { note: "Framework every pack needs" }),
        component("F.Fish", {
          manual: { url: "https://example.com/f", instructions: "the zip" },
        }),
        component("L.Lookup", { version_rule: "at_least", client_only: true }),
      ]),
      groups: [{ name: "Portraits", description: "Pick", choose: "one" }],
      collection: {
        id: "orig",
        name: "Cozy Co-op",
        author: "Ann",
        revision: 2,
        notes: "",
      },
    };
    const draft = forkDraft(parent, "fork");
    expect(draft.forkedFrom).toEqual({
      id: "orig",
      revision: 2,
      name: "Cozy Co-op",
      author: "Ann",
    });
    expect(draft.name).toBe("Cozy Co-op (my version)");
    expect(draft.groups).toEqual(parent.groups);
    expect(draft.mods["a.portrait"]).toEqual({ group: "Portraits" });
    expect(draft.mods["c.patcher"].note).toBe("Framework every pack needs");
    expect(draft.mods["f.fish"].manualUrl).toBe("https://example.com/f");
    expect(draft.mods["l.lookup"]).toMatchObject({
      newerOk: true,
      clientOnly: true,
    });
    // Rebuilt from the same mods, the fork's terms match the original's.
    const rebuilt = buildCollectionRecipe(
      overview,
      parent.components.map((c) => mod(c.unique_id)),
      draft,
      1,
      "now",
    );
    expect(diffRecipes(parent, rebuilt).termsChanged).toEqual([]);
    expect(rebuilt.collection?.forked_from?.author).toBe("Ann");
  });
});
