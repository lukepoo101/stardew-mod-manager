import { describe, expect, it } from "vitest";
import { listUnknowns } from "@/features/diagnostics/UnknownsCard";
import type {
  DiagnosticsDto,
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

describe("what the manager does not know", () => {
  it("gathers unknowns with why and what to do, and nothing else", () => {
    const unknowns = listUnknowns({
      overview: {
        smapi_status: { is_installed: true, comparison: "unknown" },
        health_summary: {
          findings: [
            {
              code: "RUNTIME_PAIR_UNASSESSED",
              title: "Could not check SMAPI against the game version",
              summary: "Versions could not be compared.",
            },
            { code: "MISSING_DEPENDENCY", title: "x", summary: "y" },
          ],
        },
      } as unknown as ProfileOverviewDto,
      report: { log_match: "unknown" } as unknown as DiagnosticsDto,
      mods: [{ artifact_hash: "" }] as unknown as ModListItemDto[],
      dismissed: [{ fingerprint: "f", signature: "s", previous: null }],
    });
    const whats = unknowns.map((u) => u.what);
    expect(whats).toContain("Which SMAPI version is installed");
    expect(whats).toContain("Could not check SMAPI against the game version");
    expect(whats).toContain("Where 1 mod(s) came from");
    expect(whats).toContain("Which session the current SMAPI log belongs to");
    expect(whats).toContain("1 finding(s) you dismissed");
    // A known problem is not an unknown.
    expect(whats).not.toContain("x");
    expect(unknowns.every((u) => u.why && u.next)).toBe(true);
  });
});
