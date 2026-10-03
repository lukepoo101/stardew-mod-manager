import { describe, expect, it, vi } from "vitest";
import type { FindingDto } from "@/shared/api/generated";
import { linksIn, revisionNotes } from "@/shared/recipe/notes";
import { chosenAlready, requirementsFor } from "@/shared/recipe/recommendation";

const missing = (dependent: string, required: string, minimum = "") =>
  ({
    code: "MISSING_DEPENDENCY",
    title: `Missing Required Dependency '${required}'`,
    summary: `Mod '${dependent}' requires '${required}'${minimum ? ` ${minimum} or newer` : ""}, but it is not installed.`,
    affected_entities: [dependent],
  }) as FindingDto;

describe("a recommended mod's own requirements", () => {
  it("installs only requirements with exactly one fitting stored package", async () => {
    const find = vi.fn(async (id: string) =>
      id === "Core.Lib"
        ? [
            {
              artifact_hash: "h1",
              name: "Core",
              version: "2.0",
              original_filename: "core.zip",
              meets_minimum: true,
            },
          ]
        : [],
    );
    const plan = await requirementsFor(
      "Fancy.Mod",
      [
        missing("Fancy.Mod", "Core.Lib", "1.5"),
        missing("Fancy.Mod", "Rare.Lib"),
        missing("Other.Mod", "Never.Asked"),
      ],
      find,
    );
    expect(plan.install.map((r) => r.candidate.artifact_hash)).toEqual(["h1"]);
    expect(plan.unresolved).toEqual(["Rare.Lib"]);
    expect(find).toHaveBeenCalledWith("Core.Lib", "1.5");
    expect(find).not.toHaveBeenCalledWith("Never.Asked", expect.anything());
  });

  it("names what is already chosen from a pick-one group", () => {
    const members = [
      { unique_id: "A", group: "Portraits" },
      { unique_id: "B", group: "Portraits" },
      { unique_id: "C" },
    ];
    const installed = new Set(["a", "c"]);
    expect(
      chosenAlready(
        { name: "Portraits", choose: "one" },
        members,
        installed,
        "B",
      ),
    ).toEqual(["A"]);
    expect(
      chosenAlready(
        { name: "Portraits", choose: "any" },
        members,
        installed,
        "B",
      ),
    ).toEqual([]);
  });
});

describe("curator notes", () => {
  it("finds links without trailing punctuation and reads revision notes", () => {
    expect(
      linksIn(
        "See https://example.com/a. Also http://x.org/b, and again https://example.com/a",
      ),
    ).toEqual(["https://example.com/a", "http://x.org/b"]);
    expect(revisionNotes('{"collection":{"notes":"Hi"}}')).toBe("Hi");
    expect(revisionNotes("{broken")).toBe("");
  });
});
