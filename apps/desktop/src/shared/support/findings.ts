import type { FindingDto } from "@/shared/api/generated";

export type SeverityKey = "error" | "warning" | "info";

/**
 * Backend severities are free-form strings and arrive in either case. Anything
 * that is not recognised is treated as an error so an unfamiliar finding is
 * never filed under a harmless or hidden heading.
 */
export function severityKey(severity: string): SeverityKey {
  const value = severity.trim().toLowerCase();
  if (value === "warning") return "warning";
  if (value === "info" || value === "information" || value === "informational")
    return "info";
  return "error";
}

export interface FindingFilter {
  severities: ReadonlySet<SeverityKey>;
  categories: ReadonlySet<string>;
}

export const EMPTY_FILTER: FindingFilter = {
  severities: new Set(),
  categories: new Set(),
};

/** An empty facet means "no restriction", so clearing every filter shows all. */
export function filterFindings(
  findings: readonly FindingDto[],
  filter: FindingFilter,
): FindingDto[] {
  return findings.filter(
    (finding) =>
      (filter.severities.size === 0 ||
        filter.severities.has(severityKey(finding.severity))) &&
      (filter.categories.size === 0 || filter.categories.has(finding.category)),
  );
}

export function findingCounts(findings: readonly FindingDto[]) {
  const severities: Record<SeverityKey, number> = {
    error: 0,
    warning: 0,
    info: 0,
  };
  const categories = new Map<string, number>();
  for (const finding of findings) {
    severities[severityKey(finding.severity)] += 1;
    categories.set(
      finding.category,
      (categories.get(finding.category) ?? 0) + 1,
    );
  }
  return { severities, categories };
}
