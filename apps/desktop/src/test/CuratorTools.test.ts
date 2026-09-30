import { describe, expect, it } from "vitest";
import {
  checkRecipe,
  compareVersions,
  diffRecipes,
  isEmptyChangelog,
  renderChangelog,
} from "@/shared/recipe/curator";
import type { ProfileRecipe, RecipeComponent } from "@/shared/recipe/recipe";

const hash = (c: string) => c.repeat(64);

const component = (
  id: string,
  version: string,
  over: Partial<RecipeComponent> = {},
): RecipeComponent => ({
  unique_id: id,
  name: id,
  author: "a",
  version,
  enabled: true,
  artifact_hash: hash(
    id
      .charAt(0)
      .toLowerCase()
      .replace(/[^a-f]/, "a"),
  ),
  optional: false,
  ...over,
});

const recipe = (
  components: RecipeComponent[],
  over: Partial<ProfileRecipe> = {},
): ProfileRecipe => ({
  schema: "stardew-mod-manager.profile-recipe",
  schema_version: 1,
  generated_at: "",
  profile_name: "Co-op",
  game: { storefront: "Steam", smapi_version: "4.1.10" },
  components,
  ...over,
});

describe("curator checks", () => {
  it("passes a fully pinned recipe and scores it 100", () => {
    const report = checkRecipe(
      recipe([
        component("A.Mod", "1.0", { optional: true }),
        component("B.Mod", "2.0"),
      ]),
    );
    expect(report.publishable).toBe(true);
    expect(report.reproducibility).toBe(100);
    expect(report.checks.filter((c) => c.status === "fail")).toHaveLength(0);
  });

  it("fails an empty recipe and duplicate ids", () => {
    expect(checkRecipe(recipe([])).publishable).toBe(false);
    const dup = checkRecipe(
      recipe([component("A.Mod", "1.0"), component("a.mod", "2.0")]),
    );
    expect(dup.publishable).toBe(false);
    expect(dup.checks.find((c) => c.id === "duplicates")?.detail).toMatch(
      /a\.mod/i,
    );
  });

  it("warns, and lowers the score, for mods without a checksum or version", () => {
    const report = checkRecipe(
      recipe([
        component("A.Mod", "1.0"),
        component("B.Mod", "2.0", { artifact_hash: "" }),
        component("C.Mod", "", {}),
      ]),
    );
    expect(report.reproducibility).toBe(33);
    expect(report.checks.find((c) => c.id === "packages")?.status).toBe("warn");
    expect(report.checks.find((c) => c.id === "versions")?.status).toBe("fail");
    expect(report.publishable).toBe(false);
  });

  it("says when the SMAPI version and optional marks are missing", () => {
    const report = checkRecipe(
      recipe([component("A.Mod", "1.0")], {
        game: { storefront: "", smapi_version: null },
      }),
    );
    expect(report.checks.find((c) => c.id === "smapi")?.status).toBe("warn");
    expect(report.checks.find((c) => c.id === "optional")?.status).toBe("warn");
  });

  it("does not claim safety anywhere in its wording", () => {
    const report = checkRecipe(recipe([component("A.Mod", "1.0")]));
    for (const check of report.checks) {
      expect(`${check.label} ${check.detail}`).not.toMatch(
        /\bsafe\b|trusted|verified/i,
      );
    }
  });
});

describe("version comparison", () => {
  it.each([
    ["1.0", "1.0.0", 0],
    ["1.2", "1.10", -1],
    ["2.0.0", "1.9.9", 1],
    ["1.0.0-beta", "1.0.0", 1],
  ])("%s vs %s", (a, b, expected) => {
    expect(compareVersions(a, b)).toBe(expected);
  });
});

describe("changelog", () => {
  const previous = recipe([
    component("A.Keep", "1.0"),
    component("B.Up", "1.0"),
    component("C.Down", "2.0"),
    component("D.Gone", "1.0"),
    component("E.Repack", "1.0", { artifact_hash: hash("e") }),
    component("F.Toggle", "1.0"),
  ]);
  const next = recipe(
    [
      component("A.Keep", "1.0"),
      component("B.Up", "1.5"),
      component("C.Down", "1.9"),
      component("E.Repack", "1.0", { artifact_hash: hash("f") }),
      component("F.Toggle", "1.0", { enabled: false }),
      component("G.New", "3.0"),
    ],
    { profile_name: "Co-op v2" },
  );
  const log = diffRecipes(previous, next);

  it("classifies every kind of change", () => {
    expect(log.added.map((c) => c.unique_id)).toEqual(["G.New"]);
    expect(log.removed.map((c) => c.unique_id)).toEqual(["D.Gone"]);
    expect(log.upgraded[0].to.unique_id).toBe("B.Up");
    expect(log.downgraded[0].to.unique_id).toBe("C.Down");
    expect(log.repackaged[0].to.unique_id).toBe("E.Repack");
    expect(log.enabledChanged[0].to.unique_id).toBe("F.Toggle");
    expect(log.unchanged).toBe(1);
  });

  it("renders release notes and is empty for identical recipes", () => {
    const text = renderChangelog(previous, next, log);
    expect(text).toContain('Changes from "Co-op" to "Co-op v2"');
    expect(text).toContain("+ G.New (G.New) 3.0");
    expect(text).toContain("- D.Gone (D.Gone) 1.0");
    expect(text).toContain("B.Up (B.Up) 1.0 to 1.5");
    expect(text).toContain("Rolled back (1)");
    const same = diffRecipes(previous, previous);
    expect(isEmptyChangelog(same)).toBe(true);
    expect(renderChangelog(previous, previous, same)).toMatch(/No changes/);
  });
});
