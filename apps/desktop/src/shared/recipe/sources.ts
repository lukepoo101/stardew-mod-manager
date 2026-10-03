import type { ModAnnotationDto } from "@/shared/api/generated";

/** Source links the user added to mods, by lower-case UniqueID. */
export function sourceLinks(
  annotations: readonly ModAnnotationDto[] | undefined,
): Map<string, string> {
  return new Map(
    (annotations ?? [])
      .filter((a): a is ModAnnotationDto & { source_url: string } =>
        Boolean(a.source_url),
      )
      .map((a) => [a.unique_id.toLowerCase(), a.source_url]),
  );
}
