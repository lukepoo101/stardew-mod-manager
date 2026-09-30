import type { ModListItemDto, SkippedModDto } from "@/shared/api/generated";

/**
 * Matches the mods SMAPI skipped in its last log with installed mods. SMAPI
 * names a skipped mod by display name (and usually version), not UniqueID,
 * so a name alone is only a likely match.
 */
export type SkipMatch =
  | { kind: "exact"; skipped: SkippedModDto; mod: ModListItemDto }
  | { kind: "likely"; skipped: SkippedModDto; mod: ModListItemDto }
  | { kind: "unmatched"; skipped: SkippedModDto };

const norm = (value: string) => value.trim().toLowerCase();

export function matchSkipped(
  skipped: readonly SkippedModDto[],
  mods: readonly ModListItemDto[],
): SkipMatch[] {
  return skipped.map((entry) => {
    const named = mods.filter((m) => norm(m.name) === norm(entry.name));
    const exact = named.find(
      (m) => entry.version !== null && norm(m.version) === norm(entry.version),
    );
    if (exact) return { kind: "exact", skipped: entry, mod: exact };
    if (named.length === 1)
      return { kind: "likely", skipped: entry, mod: named[0] };
    return { kind: "unmatched", skipped: entry };
  });
}

/** Skip matches keyed by the installed mod they point at. */
export function skippedByComponent(
  matches: readonly SkipMatch[],
): Map<string, Extract<SkipMatch, { mod: ModListItemDto }>> {
  const out = new Map<string, Extract<SkipMatch, { mod: ModListItemDto }>>();
  for (const match of matches) {
    if (match.kind !== "unmatched") {
      out.set(match.mod.profile_component_id, match);
    }
  }
  return out;
}
