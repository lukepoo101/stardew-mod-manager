import type { LaunchSessionDto } from "@/shared/api/generated";

export interface VersionChange {
  earlier: string | null;
  later: string | null;
}

export interface SessionComparison {
  earlier: LaunchSessionDto;
  later: LaunchSessionDto;
  /** Enabled at the later start but not the earlier one. */
  added: string[];
  /** Enabled at the earlier start but not the later one. */
  removed: string[];
  game: VersionChange;
  smapi: VersionChange;
}

const lower = (ids: readonly string[]) =>
  new Map(ids.map((id) => [id.toLowerCase(), id]));

/**
 * What differed between two sessions' starts: the mods that were enabled and
 * the game and SMAPI versions. Order does not matter; the earlier session is
 * the one that started first.
 */
export function compareSessions(
  a: LaunchSessionDto,
  b: LaunchSessionDto,
): SessionComparison {
  const [earlier, later] =
    Date.parse(a.launched_at) <= Date.parse(b.launched_at) ? [a, b] : [b, a];
  const before = lower(earlier.expected_mods);
  const after = lower(later.expected_mods);
  const sorted = (ids: string[]) =>
    ids.sort((x, y) => x.localeCompare(y, undefined, { sensitivity: "base" }));
  return {
    earlier,
    later,
    added: sorted([...after].filter(([k]) => !before.has(k)).map(([, v]) => v)),
    removed: sorted(
      [...before].filter(([k]) => !after.has(k)).map(([, v]) => v),
    ),
    game: { earlier: earlier.game_version, later: later.game_version },
    smapi: { earlier: earlier.smapi_version, later: later.smapi_version },
  };
}

/** A version change in words, honest about what was not known. */
export function describeVersion(change: VersionChange): string {
  if (change.earlier === null || change.later === null) {
    return `${change.earlier ?? "unknown"} → ${change.later ?? "unknown"} (not known for both)`;
  }
  return change.earlier === change.later
    ? `${change.later} (unchanged)`
    : `${change.earlier} → ${change.later}`;
}
