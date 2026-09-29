import { describe, expect, it } from "vitest";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import {
  buildRecipe,
  compareWithRecipe,
  differenceKey,
  parseRecipe,
  renderComparison,
  serializeRecipe,
} from "@/shared/recipe/recipe";

const overview = {
  profile: { name: "Co-op" },
  game: { storefront: "Steam" },
  smapi_status: { observed_version: "4.1.10" },
} as unknown as ProfileOverviewDto;

const mod = (
  id: string,
  version: string,
  extra: Partial<ModListItemDto> = {},
) =>
  ({
    unique_id: id,
    name: id,
    author: "a",
    version,
    enabled: true,
    artifact_hash: `hash-${id}-${version}`,
    ...extra,
  }) as ModListItemDto;

const recipeFrom = (mods: ModListItemDto[]) => {
  const parsed = parseRecipe(
    serializeRecipe(buildRecipe(overview, mods, "2026-09-29T00:00:00Z")),
  );
  if (!parsed.ok) throw new Error(parsed.errors.join());
  return parsed.recipe;
};

describe("recipe round trip", () => {
  it("exports deterministically with no paths and re-imports identically", () => {
    const mods = [mod("B.Mod", "1.0"), mod("A.Mod", "2.0")];
    const a = serializeRecipe(buildRecipe(overview, mods, "t"));
    const b = serializeRecipe(buildRecipe(overview, [...mods].reverse(), "t"));
    expect(a).toBe(b);
    expect(a).not.toMatch(/\/home|C:\\\\/);
    expect(compareWithRecipe(mods, recipeFrom(mods)).identical).toBe(true);
  });
});

describe("recipe validation", () => {
  it.each([
    ["not json", "{"],
    ["an array", "[]"],
    ["another schema", JSON.stringify({ schema: "other" })],
  ])("rejects %s", (_name, text) => {
    expect(parseRecipe(text).ok).toBe(false);
  });

  it("rejects an unsupported version and says which one", () => {
    const result = parseRecipe(
      JSON.stringify({
        schema: "stardew-mod-manager.profile-recipe",
        schema_version: 99,
      }),
    );
    expect(result.ok).toBe(false);
    expect(!result.ok && result.errors[0]).toMatch(/99/);
  });

  it("does not coerce wrong types into an empty recipe", () => {
    const result = parseRecipe(
      JSON.stringify({
        schema: "stardew-mod-manager.profile-recipe",
        schema_version: 1,
        profile_name: "x",
        components: [{ unique_id: 5, enabled: "yes" }],
      }),
    );
    expect(result.ok).toBe(false);
    expect(!result.ok && result.errors.join()).toMatch(/unique_id/);
  });
});

describe("recipe comparison", () => {
  const recipe = recipeFrom([
    mod("A.Mod", "1.0.0"),
    mod("B.Mod", "2.0.0"),
    mod("C.Mod", "3.0.0"),
    mod("D.Mod", "4.0.0"),
    mod("E.Mod", "5.0.0"),
  ]);
  const installed = [
    mod("A.Mod", "1.0.0"),
    mod("B.Mod", "2.1.0"),
    mod("C.Mod", "3.0.0", { artifact_hash: "other" }),
    mod("D.Mod", "4.0.0", { enabled: false }),
    mod("Extra.Mod", "9.0.0"),
  ];
  const result = compareWithRecipe(installed, recipe);
  const kinds = Object.fromEntries(
    result.differences.map((d) => [d.unique_id, d.kind]),
  );

  it("classifies each kind of difference by exact identity", () => {
    expect(kinds).toEqual({
      "B.Mod": "version",
      "C.Mod": "package",
      "D.Mod": "enabled",
      "E.Mod": "missing",
      "Extra.Mod": "extra",
    });
    expect(result.matching).toBe(1);
    expect(result.identical).toBe(false);
  });

  it("marks accepted differences in the rendered comparison", () => {
    const accepted = new Set([differenceKey(result.differences[0])]);
    expect(renderComparison(result, accepted)).toMatch(/\(accepted\)/);
    expect(renderComparison(result, new Set())).not.toMatch(/accepted/);
  });

  it("flags duplicate ids instead of silently picking one", () => {
    const dup = compareWithRecipe(
      [mod("A.Mod", "1"), mod("A.Mod", "2")],
      recipe,
    );
    expect(dup.duplicates).toContain("A.Mod");
  });
});
