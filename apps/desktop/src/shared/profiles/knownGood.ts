import type { FrozenModDto, ModListItemDto } from "@/shared/api/generated";

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
