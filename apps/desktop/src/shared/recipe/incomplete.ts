import type {
  ModListItemDto,
  ReferenceRecipeDto,
} from "@/shared/api/generated";
import {
  compareWithRecipe,
  differenceKey,
  type IncompleteItem,
  parseRecipe,
} from "./recipe";

/**
 * Required mods the profile's own reference asks for that are not installed
 * and not accepted as left out: what makes an export of it incomplete.
 */
export function incompleteItems(
  reference: ReferenceRecipeDto | null | undefined,
  mods: readonly ModListItemDto[],
): IncompleteItem[] {
  if (!reference) return [];
  const parsed = parseRecipe(reference.recipe_json);
  if (!parsed.ok) return [];
  const accepted = new Set(reference.accepted);
  return compareWithRecipe(mods, parsed.recipe)
    .differences.filter(
      (d) =>
        d.kind === "missing" &&
        !d.optional &&
        !accepted.has(differenceKey(d)) &&
        d.recipe,
    )
    .map((d) => ({
      unique_id: d.unique_id,
      name: d.recipe?.name ?? d.unique_id,
      version: d.recipe?.version ?? "",
      artifact_hash: d.recipe?.artifact_hash ?? "",
    }));
}
