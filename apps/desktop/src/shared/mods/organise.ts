import type { ModAnnotationDto, ModListItemDto } from "@/shared/api/generated";

/**
 * Sorting and tag filtering for the Mods list. Annotations (favourites, tags,
 * notes) are presentation data keyed by UniqueID; nothing here changes what is
 * installed or loaded.
 */

export const MOD_SORTS = [
  "name",
  "author",
  "newest",
  "favourites",
  "enabled",
] as const;
export type ModSort = (typeof MOD_SORTS)[number];

export const MOD_SORT_LABELS: Record<ModSort, string> = {
  name: "Name",
  author: "Author",
  newest: "Recently installed",
  favourites: "Favourites first",
  enabled: "Enabled first",
};

export type AnnotationIndex = ReadonlyMap<string, ModAnnotationDto>;

export function indexAnnotations(
  annotations: readonly ModAnnotationDto[] | undefined,
): AnnotationIndex {
  return new Map(
    (annotations ?? []).map((a) => [a.unique_id.toLowerCase(), a] as const),
  );
}

export function annotationFor(
  index: AnnotationIndex,
  mod: Pick<ModListItemDto, "unique_id">,
): ModAnnotationDto | undefined {
  return mod.unique_id ? index.get(mod.unique_id.toLowerCase()) : undefined;
}

const byName = (a: ModListItemDto, b: ModListItemDto) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base" }) ||
  a.unique_id.localeCompare(b.unique_id);

/** A stable sort: ties always fall back to name, then UniqueID. */
export function sortMods(
  mods: readonly ModListItemDto[],
  sort: ModSort,
  index: AnnotationIndex,
): ModListItemDto[] {
  const favourite = (mod: ModListItemDto) =>
    annotationFor(index, mod)?.favourite ? 0 : 1;
  const compare: Record<
    ModSort,
    (a: ModListItemDto, b: ModListItemDto) => number
  > = {
    name: byName,
    author: (a, b) =>
      a.author.localeCompare(b.author, undefined, { sensitivity: "base" }) ||
      byName(a, b),
    newest: (a, b) =>
      (Date.parse(b.installed_at) || 0) - (Date.parse(a.installed_at) || 0) ||
      byName(a, b),
    favourites: (a, b) => favourite(a) - favourite(b) || byName(a, b),
    enabled: (a, b) => Number(b.enabled) - Number(a.enabled) || byName(a, b),
  };
  return [...mods].sort(compare[sort]);
}

/** Every tag in use, in a case-insensitive alphabetical order. */
export function allTags(index: AnnotationIndex): string[] {
  const seen = new Map<string, string>();
  for (const annotation of index.values()) {
    for (const tag of annotation.tags) {
      if (!seen.has(tag.toLowerCase())) seen.set(tag.toLowerCase(), tag);
    }
  }
  return [...seen.values()].sort((a, b) =>
    a.localeCompare(b, undefined, { sensitivity: "base" }),
  );
}

export function hasTag(
  index: AnnotationIndex,
  mod: ModListItemDto,
  tag: string,
): boolean {
  const wanted = tag.toLowerCase();
  return (
    annotationFor(index, mod)?.tags.some((t) => t.toLowerCase() === wanted) ??
    false
  );
}

/** Splits comma-separated tag input the way the backend will store it. */
export function parseTagInput(text: string): string[] {
  const seen = new Set<string>();
  const tags: string[] = [];
  for (const raw of text.split(",")) {
    const tag = raw.split(/\s+/).filter(Boolean).join(" ");
    if (tag && !seen.has(tag.toLowerCase())) {
      seen.add(tag.toLowerCase());
      tags.push(tag);
    }
  }
  return tags;
}
