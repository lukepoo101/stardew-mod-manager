import type {
  DependencyMapEntryDto,
  ModListItemDto,
} from "@/shared/api/generated";

/**
 * The enabled mods that would stop loading if `removed` went away: those
 * that require them, and those that require those, and so on. Optional
 * relationships are not followed. Read only.
 */
export function enabledDependents(
  removed: readonly string[],
  map: readonly DependencyMapEntryDto[],
  mods: readonly ModListItemDto[],
): ModListItemDto[] {
  const byName = new Map(map.map((e) => [e.name, e]));
  const gone = new Set(removed);
  const queue = [...removed];
  while (queue.length > 0) {
    const name = queue.shift() as string;
    for (const dependent of byName.get(name)?.required_by ?? []) {
      if (gone.has(dependent)) continue;
      gone.add(dependent);
      queue.push(dependent);
    }
  }
  for (const name of removed) gone.delete(name);
  return mods.filter((m) => m.enabled && gone.has(m.name));
}
