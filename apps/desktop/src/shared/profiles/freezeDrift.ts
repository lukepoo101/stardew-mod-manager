import type { FrozenModDto, ModListItemDto } from "@/shared/api/generated";

export interface FreezeDrift {
  /** Enabled or disabled since the freeze; installs and removals are blocked. */
  enabledChanged: { name: string; nowEnabled: boolean }[];
  /** Present only if the freeze was bypassed outside the manager. */
  added: string[];
  removed: string[];
}

/** How the profile differs from its frozen snapshot, matched by UniqueID. */
export function freezeDrift(
  snapshot: readonly FrozenModDto[],
  current: readonly ModListItemDto[],
): FreezeDrift {
  const key = (id: string) => id.toLowerCase();
  const now = new Map(current.map((m) => [key(m.unique_id), m]));
  const then = new Map(snapshot.map((m) => [key(m.unique_id), m]));
  const drift: FreezeDrift = { enabledChanged: [], added: [], removed: [] };
  for (const [id, frozen] of then) {
    const mod = now.get(id);
    if (!mod) drift.removed.push(frozen.name);
    else if (mod.enabled !== frozen.enabled)
      drift.enabledChanged.push({ name: mod.name, nowEnabled: mod.enabled });
  }
  for (const [id, mod] of now) {
    if (!then.has(id)) drift.added.push(mod.name);
  }
  return drift;
}

/**
 * Mods (lower-case UniqueIDs) whose settings files differ from the ones
 * recorded at the freeze: changed, added or gone. Compared by checksum.
 */
export function settingsDrift(
  frozen: readonly { unique_id: string; path: string; sha256: string }[],
  current: readonly { unique_id: string; path: string; sha256: string }[],
): string[] {
  const key = (h: { unique_id: string; path: string }) =>
    `${h.unique_id.toLowerCase()}/${h.path}`;
  const then = new Map(frozen.map((h) => [key(h), h.sha256]));
  const now = new Map(current.map((h) => [key(h), h.sha256]));
  const changed = new Set<string>();
  for (const [k, sha] of then)
    if (now.get(k) !== sha) changed.add(k.split("/")[0]);
  for (const k of now.keys()) if (!then.has(k)) changed.add(k.split("/")[0]);
  return [...changed].sort();
}

/** Whether a frozen setup is exactly the one last seen working. */
export function sameMods(
  a: readonly FrozenModDto[],
  b: readonly FrozenModDto[],
): boolean {
  const sig = (mods: readonly FrozenModDto[]) =>
    mods
      .map(
        (m) =>
          `${m.unique_id.toLowerCase()}@${m.version}#${m.artifact_hash.toLowerCase()}:${m.enabled}`,
      )
      .sort()
      .join("|");
  return sig(a) === sig(b);
}
