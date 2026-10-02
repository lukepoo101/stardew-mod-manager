import { describe, expect, it } from "vitest";
import type { FindingDto } from "@/shared/api/generated";
import {
  canDismiss,
  findingSignature,
  partitionFindings,
  snapshotOf,
} from "@/shared/support/dismissals";

const finding = (over: Partial<FindingDto> = {}): FindingDto => ({
  id: "1",
  fingerprint: "fp",
  code: "X",
  severity: "info",
  category: "runtime",
  title: "t",
  summary: "s",
  affected_entities: [],
  evidence: ["e1"],
  observed_at: "now",
  ...over,
});

describe("finding dismissals", () => {
  it("hides a finding only while its content is unchanged", () => {
    const f = finding();
    const dismissals = [{ fingerprint: "fp", signature: findingSignature(f) }];
    expect(partitionFindings([f], dismissals).dismissed).toHaveLength(1);

    const changed = finding({ evidence: ["e1", "e2"] });
    const result = partitionFindings([changed], dismissals);
    expect(result.visible).toHaveLength(1);
    expect(result.dismissed).toHaveLength(0);
  });

  it("is not affected by observation time or id", () => {
    const f = finding();
    const later = finding({ id: "2", observed_at: "later" });
    expect(findingSignature(f)).toBe(findingSignature(later));
  });

  it("never hides errors, whatever is recorded", () => {
    const f = finding({ severity: "Error" });
    expect(canDismiss(f)).toBe(false);
    const dismissals = [{ fingerprint: "fp", signature: findingSignature(f) }];
    expect(partitionFindings([f], dismissals).visible).toHaveLength(1);
    expect(canDismiss(finding({ severity: "critical" }))).toBe(false);
    expect(canDismiss(finding({ severity: "mystery" }))).toBe(false);
  });

  it("leaves other findings alone", () => {
    const a = finding({ fingerprint: "a" });
    const b = finding({ fingerprint: "b" });
    const dismissals = [{ fingerprint: "a", signature: findingSignature(a) }];
    const result = partitionFindings([a, b], dismissals);
    expect(result.visible.map((f) => f.fingerprint)).toEqual(["b"]);
  });

  it("says what changed when a dismissed finding comes back", () => {
    const before = finding({ severity: "info" });
    const dismissals = [
      {
        fingerprint: "fp",
        signature: findingSignature(before),
        previous: snapshotOf(before),
      },
    ];
    const now = finding({ severity: "warning", evidence: ["e2"] });
    const result = partitionFindings([now], dismissals);
    expect(result.returned.get("fp")).toEqual([
      "Severity was info, now warning.",
      "New evidence: e2",
      "No longer seen: e1",
    ]);
    // Still hidden: nothing to explain.
    expect(partitionFindings([before], dismissals).returned.size).toBe(0);
    // An older dismissal without a snapshot still says that it changed.
    const legacy = [{ fingerprint: "fp", signature: "old" }];
    expect(partitionFindings([now], legacy).returned.get("fp")).toEqual([
      "What it reports changed since you dismissed it.",
    ]);
  });
});
