import type {
  BaselineFindingDto,
  FindingDto,
  FrozenModDto,
  ModListItemDto,
} from "@/shared/api/generated";

export interface KnownGoodDiff {
  /** Enabled then, disabled now, or the other way round. */
  enabledChanged: { mod: ModListItemDto; wasEnabled: boolean }[];
  /** Installed since; restoring disables them. */
  added: ModListItemDto[];
  /** Gone since; they have to be installed again by hand. */
  removed: FrozenModDto[];
  /** A different version now; restoring cannot change versions. */
  versionChanged: { mod: ModListItemDto; was: string }[];
}

/** How the profile differs from when it last worked, matched by UniqueID. */
export function knownGoodDiff(
  snapshot: readonly FrozenModDto[],
  current: readonly ModListItemDto[],
): KnownGoodDiff {
  const key = (id: string) => id.toLowerCase();
  const then = new Map(snapshot.map((m) => [key(m.unique_id), m]));
  const now = new Map(current.map((m) => [key(m.unique_id), m]));
  const diff: KnownGoodDiff = {
    enabledChanged: [],
    added: [],
    removed: [],
    versionChanged: [],
  };
  for (const [id, mod] of now) {
    const was = then.get(id);
    if (!was) {
      diff.added.push(mod);
      continue;
    }
    if (was.version !== mod.version) {
      diff.versionChanged.push({ mod, was: was.version });
    }
    if (was.enabled !== mod.enabled) {
      diff.enabledChanged.push({ mod, wasEnabled: was.enabled });
    }
  }
  for (const [id, was] of then) {
    if (!now.has(id)) diff.removed.push(was);
  }
  return diff;
}

/** Which mods to enable and disable to get the enabled state back. */
export function restorePlan(diff: KnownGoodDiff): {
  enable: string[];
  disable: string[];
} {
  const enable = diff.enabledChanged
    .filter((c) => c.wasEnabled)
    .map((c) => c.mod.profile_component_id);
  const disable = [
    ...diff.enabledChanged
      .filter((c) => !c.wasEnabled)
      .map((c) => c.mod.profile_component_id),
    ...diff.added.filter((m) => m.enabled).map((m) => m.profile_component_id),
  ];
  return { enable, disable };
}

export interface HealthChanges {
  /** Findings now that were not there when the profile last worked. */
  introduced: FindingDto[];
  /** Findings then that are gone now. */
  resolved: BaselineFindingDto[];
  /** The same finding, now more severe. */
  escalated: { finding: FindingDto; was: string }[];
  /** Findings present then and now at the same or lower severity. */
  unchanged: number;
}

const SEVERITY_RANK: Record<string, number> = { info: 0, warning: 1, error: 2 };
const rank = (severity: string) => SEVERITY_RANK[severity.toLowerCase()] ?? 1;

/** How health findings differ from when the profile last worked, matched by fingerprint. */
export function healthChanges(
  baseline: readonly BaselineFindingDto[],
  current: readonly FindingDto[],
): HealthChanges {
  const then = new Map(baseline.map((f) => [f.fingerprint, f]));
  const now = new Set(current.map((f) => f.fingerprint));
  const changes: HealthChanges = {
    introduced: [],
    resolved: baseline.filter((f) => !now.has(f.fingerprint)),
    escalated: [],
    unchanged: 0,
  };
  for (const finding of current) {
    const before = then.get(finding.fingerprint);
    if (!before) changes.introduced.push(finding);
    else if (rank(finding.severity) > rank(before.severity))
      changes.escalated.push({ finding, was: before.severity });
    else changes.unchanged += 1;
  }
  return changes;
}
