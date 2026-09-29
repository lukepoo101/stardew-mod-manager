import type { DismissedFindingDto, FindingDto } from "@/shared/api/generated";
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

export interface Partitioned {
  visible: FindingDto[];
  dismissed: FindingDto[];
}

export function partitionFindings(
  findings: readonly FindingDto[],
  dismissals: readonly DismissedFindingDto[],
): Partitioned {
  const signatures = new Map(
    dismissals.map((entry) => [entry.fingerprint, entry.signature]),
  );
  const result: Partitioned = { visible: [], dismissed: [] };
  for (const finding of findings) {
    const hidden =
      canDismiss(finding) &&
      signatures.get(finding.fingerprint) === findingSignature(finding);
    (hidden ? result.dismissed : result.visible).push(finding);
  }
  return result;
}
