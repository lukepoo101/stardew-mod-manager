import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import {
  buildRecipe,
  type CollectionInfo,
  type OptionGroup,
  type ProfileRecipe,
} from "./recipe";

/** The curator's per-mod choices for a collection. */
export interface ModChoice {
  optional?: boolean;
  group?: string;
  /** A newer version than the one listed also satisfies it. */
  newerOk?: boolean;
  manualUrl?: string;
  manualInstructions?: string;
  /** Only matters on each player's own computer. */
  clientOnly?: boolean;
  /** Kept on purpose even if nothing requires it any more. */
  intended?: boolean;
  /** Why it is included, shown to recipients as the curator's words. */
  note?: string;
}

/** The working state of a collection, saved per profile. */
export interface CollectionDraft {
  id: string;
  name: string;
  author: string;
  /** What changed in the next revision, for recipients. */
  notes: string;
  forkedFrom?: CollectionInfo["forked_from"];
  groups: OptionGroup[];
  /** Keyed by lower-case UniqueID. */
  mods: Record<string, ModChoice>;
  /** The last clean-profile test the curator started. */
  cleanTest?: { revision: number; at: string; profileName: string };
}

export function newDraft(name: string, id: string): CollectionDraft {
  return { id, name, author: "", notes: "", groups: [], mods: {} };
}

/** Reads a saved draft, falling back to a fresh one if it is unreadable. */
export function readDraft(
  json: string | null,
  fallback: CollectionDraft,
): CollectionDraft {
  if (!json) return fallback;
  try {
    const value = JSON.parse(json) as Partial<CollectionDraft>;
    if (typeof value.id !== "string" || typeof value.name !== "string") {
      return fallback;
    }
    return {
      id: value.id,
      name: value.name,
      author: typeof value.author === "string" ? value.author : "",
      notes: typeof value.notes === "string" ? value.notes : "",
      forkedFrom: value.forkedFrom,
      groups: Array.isArray(value.groups) ? value.groups : [],
      mods: value.mods && typeof value.mods === "object" ? value.mods : {},
    };
  } catch {
    return fallback;
  }
}

/**
 * The profile as a collection recipe for one revision: the exact mods with
 * the curator's choices applied. Groups that no mod uses are left out.
 */
export function buildCollectionRecipe(
  overview: ProfileOverviewDto,
  mods: readonly ModListItemDto[],
  draft: CollectionDraft,
  revision: number,
  generatedAt: string,
): ProfileRecipe {
  const recipe = buildRecipe(overview, mods, generatedAt);
  const groupNames = new Set(draft.groups.map((g) => g.name));
  recipe.components = recipe.components.map((component) => {
    const choice = draft.mods[component.unique_id.toLowerCase()] ?? {};
    const group =
      choice.group && groupNames.has(choice.group) ? choice.group : undefined;
    const url = choice.manualUrl?.trim();
    return {
      ...component,
      // A mod in a group is something to choose, so it is optional.
      optional: Boolean(choice.optional || group),
      ...(choice.newerOk ? { version_rule: "at_least" as const } : {}),
      ...(choice.clientOnly ? { client_only: true } : {}),
      ...(choice.note?.trim() ? { note: choice.note.trim() } : {}),
      ...(group ? { group } : {}),
      ...(url && /^https?:\/\//i.test(url)
        ? {
            manual: {
              url,
              instructions: choice.manualInstructions?.trim() ?? "",
            },
          }
        : {}),
    };
  });
  recipe.profile_name = draft.name || recipe.profile_name;
  const used = new Set(recipe.components.map((c) => c.group).filter(Boolean));
  const groups = draft.groups.filter((g) => used.has(g.name));
  recipe.collection = {
    id: draft.id,
    name: draft.name || overview.profile.name,
    author: draft.author,
    revision,
    notes: draft.notes,
    ...(draft.forkedFrom ? { forked_from: draft.forkedFrom } : {}),
  };
  if (groups.length > 0) recipe.groups = groups;
  return recipe;
}
