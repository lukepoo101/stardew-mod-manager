import { api } from "@/shared/api/client";
import type { ModListItemDto } from "@/shared/api/generated";
import {
  compareWithRecipe,
  differenceKey,
  type ProfileRecipe,
} from "@/shared/recipe/recipe";
import { settingsDifferences } from "@/shared/recipe/settings";

/**
 * The keys of every difference a profile has from a recipe, settings
 * included: what an accepted difference must still be among to stay
 * accepted when the reference is replaced.
 */
export async function differenceKeys(
  profileId: string,
  mods: readonly ModListItemDto[],
  recipe: ProfileRecipe,
): Promise<string[]> {
  const keys = compareWithRecipe(mods, recipe).differences.map(differenceKey);
  if (recipe.components.some((c) => c.settings?.length)) {
    const hashes = await api.settingsHashes(profileId).catch(() => []);
    const installed = new Set(mods.map((m) => m.unique_id.toLowerCase()));
    keys.push(
      ...settingsDifferences(recipe, hashes, installed).map((d) => d.key),
    );
  }
  return keys;
}
