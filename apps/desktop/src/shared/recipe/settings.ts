import type { SettingFileHashDto } from "@/shared/api/generated";
import type { ProfileRecipe, RecipeSetting } from "./recipe";

export interface SettingsDifference {
  unique_id: string;
  name: string;
  /** Shared files whose contents differ here, or that are missing here. */
  files: string[];
  /** The recipe's settings for the mod, to apply. */
  settings: RecipeSetting[];
  /** Identifies this exact difference: it changes if either side changes. */
  key: string;
}

/**
 * Where the profile's settings differ from the ones a recipe shares, by
 * checksum. Only mods installed here are compared; values are never read.
 */
export function settingsDifferences(
  recipe: ProfileRecipe,
  hashes: readonly SettingFileHashDto[],
  installed: ReadonlySet<string>,
): SettingsDifference[] {
  const local = new Map(
    hashes.map((h) => [`${h.unique_id.toLowerCase()}/${h.path}`, h.sha256]),
  );
  const out: SettingsDifference[] = [];
  for (const component of recipe.components) {
    const id = component.unique_id.toLowerCase();
    if (!component.settings?.length || !installed.has(id)) continue;
    const files = component.settings
      .filter(
        (s) =>
          local.get(`${id}/${s.path}`)?.toLowerCase() !==
          s.sha256.toLowerCase(),
      )
      .map((s) => s.path);
    if (files.length === 0) continue;
    const mine = files.map((path) => local.get(`${id}/${path}`) ?? "none");
    out.push({
      unique_id: component.unique_id,
      name: component.name || component.unique_id,
      files,
      settings: component.settings,
      key: `settings:${component.unique_id}:${component.settings
        .map((s) => s.sha256.slice(0, 12))
        .join("+")}:${mine.map((h) => h.slice(0, 12)).join("+")}`,
    });
  }
  return out;
}
