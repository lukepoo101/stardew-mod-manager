import { describe, expect, it } from "vitest";
import { matchSkipped } from "@/shared/diagnostics/skipped";
import type { ModListItemDto, SkippedModDto } from "@/shared/api/generated";

const mod = (id: string, name: string, version: string) =>
  ({ profile_component_id: id, name, version }) as ModListItemDto;
const skip = (name: string, version: string | null): SkippedModDto => ({
  name,
  version,
  reason: "needs X",
  missing_dependencies: [],
  line: 12,
});

describe("matching skipped mods", () => {
  it("separates exact, likely and unmatched entries", () => {
    const mods = [
      mod("c1", "Content Patcher", "2.0.0"),
      mod("c2", "Lookup", "1.0"),
    ];
    const result = matchSkipped(
      [
        skip("content patcher", "2.0.0"),
        skip("Lookup", "0.9"),
        skip("Ghost", null),
      ],
      mods,
    );
    expect(result.map((r) => r.kind)).toEqual(["exact", "likely", "unmatched"]);
  });

  it("does not guess between two installed mods with the same name", () => {
    const result = matchSkipped(
      [skip("Twin", null)],
      [mod("a", "Twin", "1.0"), mod("b", "Twin", "2.0")],
    );
    expect(result[0].kind).toBe("unmatched");
  });
});
