import type {
  DiagnosticsDto,
  FindingDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

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
