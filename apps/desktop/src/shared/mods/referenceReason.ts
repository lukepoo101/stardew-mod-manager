import type { ReferenceRecipeDto } from "@/shared/api/generated";

interface RecipeShape {
  profile_name?: string;
  components?: Array<{ unique_id?: string; optional?: boolean }>;
}

/**
 * Why a mod is in the profile when the profile follows a group reference
 * that lists it, or null when the reference does not mention it. An
 * unreadable reference gives no reason rather than a guessed one.
 */
export function referenceReason(
  reference: ReferenceRecipeDto | null | undefined,
  uniqueId: string,
): string | null {
  if (!reference) return null;
  let recipe: RecipeShape;
  try {
    recipe = JSON.parse(reference.recipe_json) as RecipeShape;
  } catch {
    return null;
  }
  const wanted = uniqueId.toLowerCase();
  const entry = recipe.components?.find(
    (c) => c.unique_id?.toLowerCase() === wanted,
  );
  if (!entry) return null;
  const from = recipe.profile_name ? ` from "${recipe.profile_name}"` : "";
  return `Listed${entry.optional ? " as optional" : ""} in the group reference${from} this profile follows (attached ${new Date(
    reference.attached_at,
  ).toLocaleDateString()}).`;
}
