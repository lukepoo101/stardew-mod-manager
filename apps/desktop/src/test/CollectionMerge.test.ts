import { describe, expect, it } from "vitest";
import type { ModListItemDto } from "@/shared/api/generated";
import { planMerge } from "@/shared/recipe/merge";
import type { ProfileRecipe, RecipeComponent } from "@/shared/recipe/recipe";

const c = (id: string, version: string, over: Partial<RecipeComponent> = {}) =>
  ({
    unique_id: id,
    name: id,
    author: "a",
    version,
    enabled: true,
    artifact_hash: "",
    optional: false,
    ...over,
  }) as RecipeComponent;
const r = (components: RecipeComponent[]) =>
  ({ components }) as unknown as ProfileRecipe;
const m = (id: string, version: string, enabled = true) =>
  ({
    unique_id: id,
    name: id,
    version,
    enabled,
    artifact_hash: "",
  }) as ModListItemDto;

describe("updating a customised profile to a new revision", () => {
  it("separates upstream changes, conflicts and your own changes", () => {
    const base = r([
      c("Up", "1.0"),
      c("Both", "1.0"),
      c("Mine", "1.0"),
      c("Gone", "1.0"),
    ]);
    const theirs = r([
      c("Up", "2.0"),
      c("Both", "2.0"),
      c("Mine", "1.0"),
      c("New", "1.0"),
    ]);
    const mine = [
      m("Up", "1.0"),
      m("Both", "1.5"),
      m("Mine", "1.0", false),
      m("Gone", "1.0"),
    ];
    const plan = planMerge(base, theirs, mine);
    const by = (id: string) => plan.find((i) => i.unique_id === id);
    expect(by("Both")?.kind).toBe("conflict");
    expect(by("Both")?.upstream).toBe("1.0 → 2.0");
    expect(by("Both")?.local).toBe("1.0 → 1.5");
    expect(by("Up")?.kind).toBe("upstream");
    expect(by("New")?.kind).toBe("upstream");
    expect(by("Gone")?.kind).toBe("upstream");
    expect(by("Gone")?.upstream).toBe("removed");
    expect(by("Mine")?.kind).toBe("local");
    expect(by("Mine")?.local).toBe("turned off");
    expect(plan[0].kind).toBe("conflict");
  });

  it("leaves out changes you already have and optional additions", () => {
    const base = r([c("Same", "1.0")]);
    const theirs = r([c("Same", "2.0"), c("Opt", "1.0", { optional: true })]);
    expect(planMerge(base, theirs, [m("Same", "2.0")])).toEqual([]);
  });
});
