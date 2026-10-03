import { describe, expect, it } from "vitest";
import { referenceReason } from "@/shared/mods/referenceReason";

const reference = (components: unknown[]) => ({
  recipe_json: JSON.stringify({ profile_name: "Farm Friends", components }),
  attached_at: "2026-10-01T10:00:00Z",
  accepted: [],
  accepted_notes: {},
});

describe("group reference as an install reason", () => {
  it("names the reference and whether the mod is optional there", () => {
    expect(
      referenceReason(reference([{ unique_id: "A.Mod" }]), "a.mod"),
    ).toMatch(/^Listed in the group reference from "Farm Friends"/);
    expect(
      referenceReason(
        reference([{ unique_id: "A.Mod", optional: true }]),
        "A.Mod",
      ),
    ).toMatch(/^Listed as optional/);
  });

  it("gives no reason when the mod is not listed or the reference is unreadable", () => {
    expect(
      referenceReason(reference([{ unique_id: "B" }]), "A.Mod"),
    ).toBeNull();
    expect(
      referenceReason(
        {
          recipe_json: "{bad",
          attached_at: "",
          accepted: [],
          accepted_notes: {},
        },
        "A.Mod",
      ),
    ).toBeNull();
    expect(referenceReason(null, "A.Mod")).toBeNull();
  });
});
