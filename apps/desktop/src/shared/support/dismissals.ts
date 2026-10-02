import type {
  DismissedFindingDto,
  DismissedSnapshotDto,
  FindingDto,
} from "@/shared/api/generated";
import { severityKey } from "./findings";

/**
 * A digest of what a finding currently says. A dismissal only applies while the
 * finding still has the signature it was dismissed with, so a finding whose
 * evidence changes comes back by itself.
 */
export function findingSignature(finding: FindingDto): string {
  const text = [
    finding.code,
    finding.severity,
    finding.summary,
    ...finding.evidence,
    ...finding.affected_entities,
  ].join("\u0000");
  // FNV-1a: small, deterministic and enough to notice a change.
  let hash = 0x811c9dc5;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

/** Errors and critical findings are never hideable, whatever is recorded. */
export function canDismiss(finding: FindingDto): boolean {
  return severityKey(finding.severity) !== "error";
}

/** What is kept about a finding when it is dismissed. */
export function snapshotOf(finding: FindingDto): DismissedSnapshotDto {
  return {
    severity: finding.severity,
    summary: finding.summary,
    evidence: [...finding.evidence],
  };
}

/**
 * What differs between a finding as it was dismissed and as it is now.
 * Without a kept snapshot (older dismissals) only the fact of a change is
 * known.
 */
export function describeChange(
  previous: DismissedSnapshotDto | null | undefined,
  finding: FindingDto,
): string[] {
  if (!previous) {
    return ["What it reports changed since you dismissed it."];
  }
  const changes: string[] = [];
  if (previous.severity !== finding.severity) {
    changes.push(`Severity was ${previous.severity}, now ${finding.severity}.`);
  }
  if (previous.summary !== finding.summary) {
    changes.push(`It used to say: "${previous.summary}"`);
  }
  const before = new Set(previous.evidence);
  const now = new Set(finding.evidence);
  for (const line of finding.evidence) {
    if (!before.has(line)) changes.push(`New evidence: ${line}`);
  }
  for (const line of previous.evidence) {
    if (!now.has(line)) changes.push(`No longer seen: ${line}`);
  }
  if (changes.length === 0) {
    changes.push("The affected items changed since you dismissed it.");
  }
  return changes;
}

type Dismissal = Pick<DismissedFindingDto, "fingerprint" | "signature"> & {
  previous?: DismissedSnapshotDto | null;
};

export interface Partitioned {
  visible: FindingDto[];
  dismissed: FindingDto[];
  /**
   * Visible findings that were dismissed before and came back because they
   * changed, by fingerprint, with what changed.
   */
  returned: Map<string, string[]>;
}

export function partitionFindings(
  findings: readonly FindingDto[],
  dismissals: readonly Dismissal[],
): Partitioned {
  const byFingerprint = new Map(
    dismissals.map((entry) => [entry.fingerprint, entry]),
  );
  const result: Partitioned = {
    visible: [],
    dismissed: [],
    returned: new Map(),
  };
  for (const finding of findings) {
    const dismissal = byFingerprint.get(finding.fingerprint);
    const hidden =
      canDismiss(finding) && dismissal?.signature === findingSignature(finding);
    (hidden ? result.dismissed : result.visible).push(finding);
    if (dismissal && !hidden && canDismiss(finding)) {
      result.returned.set(
        finding.fingerprint,
        describeChange(dismissal.previous, finding),
      );
    }
  }
  return result;
}
