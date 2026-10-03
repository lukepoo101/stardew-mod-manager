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

describe("simulating a new recipient", () => {
  it("sorts required mods by how a recipient gets them, and lists choices", async () => {
    const { simulateRecipient } = await import(
      "@/shared/recipe/collectionChecks"
    );
    const recipe = {
      components: [
        {
          unique_id: "A",
          name: "A",
          optional: false,
          artifact_hash: "x",
          manual: { url: "https://f", instructions: "" },
        },
        {
          unique_id: "B",
          name: "B",
          optional: false,
          artifact_hash: "x",
          update_keys: ["Nexus:1"],
        },
        { unique_id: "C", name: "C", optional: false, artifact_hash: "" },
        {
          unique_id: "D",
          name: "D",
          optional: true,
          group: "Looks",
          artifact_hash: "x",
        },
        {
          unique_id: "E",
          name: "E",
          optional: true,
          artifact_hash: "x",
          settings: [{ path: "config.json", sha256: "", content: "" }],
        },
      ],
      groups: [{ name: "Looks", description: "", choose: "one" }],
    } as never;
    const sim = simulateRecipient(recipe);
    expect(sim.required).toBe(3);
    expect(sim.manualLinks).toEqual(["A"]);
    expect(sim.publishedAt).toEqual(["B (Nexus:1)"]);
    expect(sim.unlocated).toEqual(["C"]);
    expect(sim.unverifiable).toEqual(["C"]);
    expect(sim.withSettings).toEqual(["E"]);
    expect(sim.choices).toEqual([
      { name: "Looks", pickOne: true, options: ["D"] },
    ]);
    expect(sim.optionalAlone).toEqual(["E"]);
    expect(sim.assumptions.join(" ")).toMatch(/Nothing online is checked/);
  });
});
