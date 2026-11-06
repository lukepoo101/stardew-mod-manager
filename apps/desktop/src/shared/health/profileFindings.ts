import type {
  DiagnosticsDto,
  DismissedFindingDto,
  FindingDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import { partitionFindings } from "@/shared/support/dismissals";
import { severityKey } from "@/shared/support/findings";

/**
 * Every finding about the active profile, from the two places they come
 * from: the profile's health checks (the same set the header and Overview
 * count) and what the latest SMAPI log says. Diagnostics, the unknowns card
 * and the support export all read this, so none of them can report "no
 * known issues" while the header shows errors.
 */
export interface ProfileFindings {
  /** From the profile's health checks. */
  health: FindingDto[];
  /** From the SMAPI log, without any the health checks already report. */
  log: FindingDto[];
  /** Both, health first. */
  all: FindingDto[];
  /** Whether both sources have answered, so an empty list means something. */
  complete: boolean;
}

export function profileFindings(
  overview: ProfileOverviewDto | undefined,
  report: DiagnosticsDto | undefined,
): ProfileFindings {
  const health = [...(overview?.health_summary?.findings ?? [])];
  const seen = new Set(health.map((finding) => finding.fingerprint));
  const log = (report?.findings ?? []).filter(
    (finding) => !seen.has(finding.fingerprint),
  );
  return {
    health,
    log,
    all: [...health, ...log],
    complete: Boolean(overview && report),
  };
}

/** A finding's time in the user's own format, rather than raw ISO text. */
export function observedAt(finding: Pick<FindingDto, "observed_at">): string {
  const at = new Date(finding.observed_at);
  return Number.isNaN(at.getTime()) ? finding.observed_at : at.toLocaleString();
}

/**
 * The profile's health counts as every page shows them: dismissed findings
 * left out (and counted apart), so the header, Overview, the Mods bar and
 * Diagnostics never disagree after a dismissal.
 */
export interface HealthCounts {
  /** The findings still shown. */
  visible: FindingDto[];
  errors: number;
  warnings: number;
  info: number;
  /** Shown information findings for checks that could not be made. */
  unassessed: number;
  dismissed: number;
}

export function visibleHealthCounts(
  findings: readonly FindingDto[],
  dismissals: readonly Pick<DismissedFindingDto, "fingerprint" | "signature">[],
): HealthCounts {
  const { visible, dismissed } = partitionFindings(findings, dismissals);
  const of = (key: string) =>
    visible.filter((finding) => severityKey(finding.severity) === key).length;
  return {
    visible,
    errors: of("error"),
    warnings: of("warning"),
    info: of("info"),
    unassessed: visible.filter((finding) =>
      finding.code.endsWith("_UNASSESSED"),
    ).length,
    dismissed: dismissed.length,
  };
}

/** "1 error(s), 2 warning(s)", with what was dismissed when any was. */
export function countsText(counts: HealthCounts, withInfo = false): string {
  return `${counts.errors} error(s), ${counts.warnings} warning(s)${
    withInfo
      ? `, ${counts.info} info${counts.unassessed > 0 ? ` (${counts.unassessed} not assessed)` : ""}`
      : ""
  }${counts.dismissed > 0 ? `, ${counts.dismissed} dismissed` : ""}`;
}
