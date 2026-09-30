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
