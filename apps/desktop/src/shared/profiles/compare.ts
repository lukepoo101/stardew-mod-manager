import type { ModListItemDto } from "@/shared/api/generated";
import { compareVersions } from "@/shared/versions";

/**
 * A read-only comparison of the mods in two installed profiles, matched by
 * UniqueID (case-insensitive, as SMAPI matches them). Nothing here changes
 * either profile.
 */

export type ProfileDifference =
  | { kind: "only_a"; a: ModListItemDto }
  | { kind: "only_b"; b: ModListItemDto }
  | {
      kind: "version";
      a: ModListItemDto;
      b: ModListItemDto;
      /** Which side has the higher version. */
      newer: "a" | "b";
    }
  /** Same version string, but installed from a different package file. */
  | { kind: "package"; a: ModListItemDto; b: ModListItemDto }
  | { kind: "enabled"; a: ModListItemDto; b: ModListItemDto };

export interface ProfileComparison {
  differences: ProfileDifference[];
  /** Same UniqueID, version, package and enabled state. */
  identical: number;
}

const key = (mod: ModListItemDto) =>
  (mod.unique_id || `component:${mod.profile_component_id}`).toLowerCase();

const nameOf = (d: ProfileDifference) =>
  ("a" in d ? d.a : d.b).name.toLowerCase();

const ORDER: ProfileDifference["kind"][] = [
  "only_a",
  "only_b",
  "version",
  "package",
  "enabled",
];

export function compareProfiles(
  a: readonly ModListItemDto[],
  b: readonly ModListItemDto[],
): ProfileComparison {
  const right = new Map(b.map((mod) => [key(mod), mod]));
  const differences: ProfileDifference[] = [];
  let identical = 0;
  const seen = new Set<string>();
  for (const left of a) {
    const id = key(left);
    seen.add(id);
    const other = right.get(id);
    if (!other) {
      differences.push({ kind: "only_a", a: left });
      continue;
    }
    const order = compareVersions(left.version, other.version);
    if (order !== 0) {
      differences.push({
        kind: "version",
        a: left,
        b: other,
        newer: order > 0 ? "a" : "b",
      });
    } else if (
      left.artifact_hash &&
      other.artifact_hash &&
      left.artifact_hash.toLowerCase() !== other.artifact_hash.toLowerCase()
    ) {
      differences.push({ kind: "package", a: left, b: other });
    } else if (left.enabled !== other.enabled) {
      differences.push({ kind: "enabled", a: left, b: other });
    } else {
      identical += 1;
    }
  }
  for (const mod of b) {
    if (!seen.has(key(mod))) differences.push({ kind: "only_b", b: mod });
  }
  differences.sort(
    (x, y) =>
      ORDER.indexOf(x.kind) - ORDER.indexOf(y.kind) ||
      nameOf(x).localeCompare(nameOf(y)),
  );
  return { differences, identical };
}

/**
 * A short summary of how profile `a` differs from profile `b`, for a warning
 * that cannot show the whole comparison. Names at most `limit` mods per kind.
 */
export function summarizeDifferences(
  comparison: ProfileComparison,
  limit = 3,
): string[] {
  const names = (mods: ModListItemDto[]) => {
    const shown = mods.slice(0, limit).map((m) => m.name);
    return mods.length > limit
      ? `${shown.join(", ")} and ${mods.length - limit} more`
      : shown.join(", ");
  };
  const of = <K extends ProfileDifference["kind"]>(kind: K) =>
    comparison.differences.filter(
      (d): d is Extract<ProfileDifference, { kind: K }> => d.kind === kind,
    );
  const lines: string[] = [];
  const onlyA = of("only_a").map((d) => d.a);
  const onlyB = of("only_b").map((d) => d.b);
  const versions = of("version");
  const enabled = of("enabled");
  if (onlyA.length > 0) lines.push(`Only in this profile: ${names(onlyA)}`);
  if (onlyB.length > 0) lines.push(`Only in the other: ${names(onlyB)}`);
  if (versions.length > 0)
    lines.push(
      `Different versions: ${versions
        .slice(0, limit)
        .map((d) => `${d.a.name} ${d.a.version} here, ${d.b.version} there`)
        .join(
          "; ",
        )}${versions.length > limit ? ` and ${versions.length - limit} more` : ""}`,
    );
  if (enabled.length > 0)
    lines.push(
      `Turned on or off differently: ${names(enabled.map((d) => d.a))}`,
    );
  return lines;
}
