import { describe, expect, it } from "vitest";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import { buildCollectionRecipe, newDraft } from "@/shared/recipe/collection";
import { checkCollection } from "@/shared/recipe/collectionChecks";

const overview = {
  profile: { id: "p1", name: "Cozy" },
  game: { storefront: "steam" },
  smapi_status: { observed_version: null },
} as unknown as ProfileOverviewDto;
const mod = (id: string, hash = "a".repeat(64)) =>
  ({
    profile_component_id: `c-${id}`,
    unique_id: id,
    name: id,
    author: "x",
    version: "1.0",
    enabled: true,
    artifact_hash: hash,
  }) as ModListItemDto;

describe("collection checks", () => {
  it("separates errors, warnings and limitations", () => {
    const draft = {
      ...newDraft("Cozy", "c1"),
      groups: [{ name: "Portraits", description: "", choose: "one" as const }],
      mods: {
        "a.mod": { group: "Portraits" },
        "b.mod": { manualUrl: "https://example.com/b" },
      },
    };
    const recipe = buildCollectionRecipe(
      overview,
      [mod("A.Mod"), mod("B.Mod", ""), mod("Lib")],
      draft,
      1,
      "now",
    );
    const result = checkCollection(recipe, draft, {
      map: [{ unique_id: "Lib", required_by: [], requires: [] } as never],
      installedAsDependency: new Set(["lib"]),
    });
    const messages = (level: string) =>
      result.checks.filter((c) => c.level === level).map((c) => c.message);
    expect(messages("error").join(" ")).toMatch(/choose one, but only one mod/);
    expect(messages("warning").join(" ")).toMatch(/has no description/);
    expect(messages("warning").join(" ")).toMatch(
      /no mod in the collection needs it now/,
    );
    expect(messages("warning").join(" ")).toMatch(/No author/);
    expect(messages("limitation").join(" ")).toMatch(/downloaded by hand/);
    expect(result.fullyAutomatic).toBe(false);
    expect(result.recipient).toMatchObject({
      manual: 1,
      optional: 1,
      noChecksum: 1,
    });
  });

  it("does not flag a requirement kept on purpose", () => {
    const draft = {
      ...newDraft("Cozy", "c1"),
      author: "Me",
      mods: { lib: { intended: true } },
    };
    const recipe = buildCollectionRecipe(
      overview,
      [mod("Lib")],
      draft,
      1,
      "now",
    );
    const result = checkCollection(recipe, draft, {
      map: [{ unique_id: "Lib", required_by: [], requires: [] } as never],
      installedAsDependency: new Set(["lib"]),
    });
    expect(result.checks.some((c) => /needs it now/.test(c.message))).toBe(
      false,
    );
    expect(result.fullyAutomatic).toBe(true);
  });
});

describe("requirements in optional contexts", () => {
  it("suggests making a requirement optional when only optional mods need it", () => {
    const draft = {
      ...newDraft("Cozy", "c1"),
      author: "Me",
      mods: { "a.mod": { optional: true } },
    };
    const recipe = buildCollectionRecipe(
      overview,
      [mod("A.Mod"), mod("Lib")],
      draft,
      1,
      "now",
    );
    const result = checkCollection(recipe, draft, {
      map: [
        { unique_id: "Lib", required_by: ["A.Mod"], requires: [] } as never,
      ],
    });
    expect(result.checks.map((c) => c.message).join(" ")).toMatch(
      /only optional mods need it \(A\.Mod\)/,
    );
  });
});

describe("locally modified mods", () => {
  it("warns that recipients get the package, not local changes", () => {
    const draft = { ...newDraft("Cozy", "c1"), author: "Me" };
    const recipe = buildCollectionRecipe(
      overview,
      [mod("A.Mod")],
      draft,
      1,
      "now",
    );
    const result = checkCollection(recipe, draft, {
      locallyModified: new Set(["a.mod"]),
    });
    expect(result.checks.find((c) => c.subject === "A.Mod")?.message).toMatch(
      /Recipients get the package as published/,
    );
  });
});
