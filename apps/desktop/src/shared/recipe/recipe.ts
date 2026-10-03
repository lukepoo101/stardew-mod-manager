import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import { compareVersions } from "./curator";

/**
 * Portable profile recipe. It describes what a profile contains by canonical
 * identity, never by local path, so it can be shared with another player or
 * moved to another operating system.
 */
export const RECIPE_SCHEMA = "stardew-mod-manager.profile-recipe";
export const RECIPE_SCHEMA_VERSION = 1;

export interface RecipeComponent {
  unique_id: string;
  name: string;
  author: string;
  version: string;
  enabled: boolean;
  /** SHA-256 of the package the exporter installed from. */
  artifact_hash: string;
  /** Marks a component the exporter considers non-essential. */
  optional: boolean;
  /** "at_least" lets a newer version satisfy it; exact otherwise. */
  version_rule?: "exact" | "at_least";
  /** The option group it belongs to. */
  group?: string;
  /** Where and how to get it by hand, for files the recipient fetches. */
  manual?: { url: string; instructions: string };
  /** Only affects the player's own computer; others need not match it. */
  client_only?: boolean;
  /** The curator's reason for including it, in their words. */
  note?: string;
  /** Settings files the curator shares for this mod: config.json text with
   * its SHA-256, so a recipient can tell whether theirs match. */
  settings?: RecipeSetting[];
}

export interface RecipeSetting {
  path: string;
  sha256: string;
  content: string;
}

/** The largest settings file a recipe may carry, as in the backend. */
export const MAX_SETTING_BYTES = 256 * 1024;

function settingProblem(setting: RecipeSetting): string | null {
  const parts = setting.path.split("/");
  if (
    !setting.path ||
    setting.path.startsWith("/") ||
    setting.path.includes("\\") ||
    parts.some((part) => part === "" || part === "." || part === "..")
  )
    return `${setting.path} is not a plain relative path`;
  if (parts.at(-1)?.toLowerCase() !== "config.json")
    return `${setting.path} is not a config.json file`;
  if (!/^[0-9a-f]{64}$/i.test(setting.sha256))
    return `${setting.path} has no valid checksum`;
  if (new TextEncoder().encode(setting.content).length > MAX_SETTING_BYTES)
    return `${setting.path} is too large to share`;
  return null;
}

export interface CollectionInfo {
  /** Stable identity across revisions. */
  id: string;
  name: string;
  author: string;
  revision: number;
  notes: string;
  forked_from?: { id: string; revision: number; name: string };
}

export interface OptionGroup {
  name: string;
  description: string;
  choose: "any" | "one";
}

export interface ProfileRecipe {
  schema: typeof RECIPE_SCHEMA;
  schema_version: number;
  generated_at: string;
  profile_name: string;
  game: { storefront: string; smapi_version: string | null };
  components: RecipeComponent[];
  /** Present for a published collection revision. */
  collection?: CollectionInfo;
  groups?: OptionGroup[];
}

export function buildRecipe(
  overview: ProfileOverviewDto,
  mods: readonly ModListItemDto[],
  generatedAt: string,
): ProfileRecipe {
  return {
    schema: RECIPE_SCHEMA,
    schema_version: RECIPE_SCHEMA_VERSION,
    generated_at: generatedAt,
    profile_name: overview.profile.name,
    game: {
      storefront: overview.game.storefront,
      smapi_version: overview.smapi_status.observed_version,
    },
    components: mods
      .map<RecipeComponent>((mod) => ({
        unique_id: mod.unique_id,
        name: mod.name,
        author: mod.author,
        version: mod.version,
        enabled: mod.enabled,
        artifact_hash: mod.artifact_hash,
        optional: false,
      }))
      .sort(
        (a, b) =>
          a.unique_id.localeCompare(b.unique_id) ||
          a.version.localeCompare(b.version),
      ),
  };
}

export function serializeRecipe(recipe: ProfileRecipe): string {
  return `${JSON.stringify(recipe, null, 2)}\n`;
}

export type ParseResult =
  | { ok: true; recipe: ProfileRecipe }
  | { ok: false; errors: string[] };

const MAX_COMPONENTS = 5000;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Validates untrusted recipe text. Nothing is coerced: a field with the wrong
 * type is an error, so a malformed recipe can never be mistaken for an empty
 * one.
 */
export function parseRecipe(text: string): ParseResult {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return { ok: false, errors: ["The file is not valid JSON."] };
  }
  if (!isObject(raw)) {
    return { ok: false, errors: ["The recipe must be a JSON object."] };
  }
  if (raw.schema !== RECIPE_SCHEMA) {
    return {
      ok: false,
      errors: ["This file is not a Stardew Mod Manager profile recipe."],
    };
  }
  if (raw.schema_version !== RECIPE_SCHEMA_VERSION) {
    return {
      ok: false,
      errors: [
        `Unsupported recipe version ${String(raw.schema_version)}; this manager reads version ${RECIPE_SCHEMA_VERSION}.`,
      ],
    };
  }

  const errors: string[] = [];
  if (typeof raw.profile_name !== "string") {
    errors.push("profile_name must be text.");
  }
  if (!Array.isArray(raw.components)) {
    errors.push("components must be a list.");
    return { ok: false, errors };
  }
  if (raw.components.length > MAX_COMPONENTS) {
    return { ok: false, errors: ["The recipe lists too many components."] };
  }

  const components: RecipeComponent[] = [];
  raw.components.forEach((entry, index) => {
    const label = `components[${index}]`;
    if (!isObject(entry)) {
      errors.push(`${label} must be an object.`);
      return;
    }
    const text = (key: string): string | null => {
      const value = entry[key];
      if (typeof value !== "string") {
        errors.push(`${label}.${key} must be text.`);
        return null;
      }
      return value;
    };
    const uniqueId = text("unique_id");
    const version = text("version");
    const name = text("name");
    const author = text("author");
    const hash = text("artifact_hash");
    if (typeof entry.enabled !== "boolean") {
      errors.push(`${label}.enabled must be true or false.`);
    }
    if (uniqueId !== null && uniqueId.trim() === "") {
      errors.push(`${label}.unique_id must not be empty.`);
    }
    let versionRule: "exact" | "at_least" | undefined;
    if (entry.version_rule !== undefined) {
      if (entry.version_rule === "exact" || entry.version_rule === "at_least") {
        versionRule = entry.version_rule;
      } else {
        errors.push(`${label}.version_rule must be exact or at_least.`);
      }
    }
    let group: string | undefined;
    if (entry.group !== undefined) {
      if (typeof entry.group === "string") group = entry.group;
      else errors.push(`${label}.group must be text.`);
    }
    let manual: { url: string; instructions: string } | undefined;
    if (entry.manual !== undefined) {
      const m = entry.manual;
      if (
        isObject(m) &&
        typeof m.url === "string" &&
        /^https?:\/\//i.test(m.url)
      ) {
        manual = {
          url: m.url,
          instructions:
            typeof m.instructions === "string" ? m.instructions : "",
        };
      } else {
        errors.push(`${label}.manual.url must be a web address.`);
      }
    }
    const settings: RecipeSetting[] = [];
    if (entry.settings !== undefined) {
      if (!Array.isArray(entry.settings)) {
        errors.push(`${label}.settings must be a list.`);
      } else {
        for (const raw of entry.settings) {
          if (
            !isObject(raw) ||
            typeof raw.path !== "string" ||
            typeof raw.sha256 !== "string" ||
            typeof raw.content !== "string"
          ) {
            errors.push(`${label}.settings has a malformed entry.`);
            continue;
          }
          const setting = {
            path: raw.path,
            sha256: raw.sha256.toLowerCase(),
            content: raw.content,
          };
          const problem = settingProblem(setting);
          if (problem) errors.push(`${label}.settings: ${problem}.`);
          else settings.push(setting);
        }
      }
    }
    if (
      uniqueId !== null &&
      version !== null &&
      name !== null &&
      author !== null &&
      hash !== null &&
      typeof entry.enabled === "boolean"
    ) {
      components.push({
        unique_id: uniqueId,
        name,
        author,
        version,
        enabled: entry.enabled,
        artifact_hash: hash,
        optional: entry.optional === true,
        ...(versionRule ? { version_rule: versionRule } : {}),
        ...(group !== undefined ? { group } : {}),
        ...(manual ? { manual } : {}),
        ...(entry.client_only === true ? { client_only: true } : {}),
        ...(typeof entry.note === "string" && entry.note.trim()
          ? { note: entry.note.slice(0, 500) }
          : {}),
        ...(settings.length > 0 ? { settings } : {}),
      });
    }
  });
  let collection: CollectionInfo | undefined;
  if (raw.collection !== undefined) {
    const c = raw.collection;
    if (
      isObject(c) &&
      typeof c.id === "string" &&
      typeof c.name === "string" &&
      typeof c.revision === "number" &&
      Number.isInteger(c.revision) &&
      c.revision > 0
    ) {
      const parent = isObject(c.forked_from) ? c.forked_from : null;
      collection = {
        id: c.id,
        name: c.name,
        author: typeof c.author === "string" ? c.author : "",
        revision: c.revision,
        notes: typeof c.notes === "string" ? c.notes : "",
        ...(parent &&
        typeof parent.id === "string" &&
        typeof parent.revision === "number"
          ? {
              forked_from: {
                id: parent.id,
                revision: parent.revision,
                name: typeof parent.name === "string" ? parent.name : "",
              },
            }
          : {}),
      };
    } else {
      errors.push(
        "collection must have an id, a name and a whole revision number.",
      );
    }
  }
  const groups: OptionGroup[] = [];
  if (raw.groups !== undefined) {
    if (!Array.isArray(raw.groups)) errors.push("groups must be a list.");
    else
      for (const g of raw.groups) {
        if (isObject(g) && typeof g.name === "string") {
          groups.push({
            name: g.name,
            description: typeof g.description === "string" ? g.description : "",
            choose: g.choose === "one" ? "one" : "any",
          });
        } else errors.push("Each group needs a name.");
      }
  }
  if (errors.length > 0) return { ok: false, errors: errors.slice(0, 10) };

  const game = isObject(raw.game) ? raw.game : {};
  return {
    ok: true,
    recipe: {
      schema: RECIPE_SCHEMA,
      schema_version: RECIPE_SCHEMA_VERSION,
      generated_at:
        typeof raw.generated_at === "string" ? raw.generated_at : "",
      profile_name: raw.profile_name as string,
      game: {
        storefront: typeof game.storefront === "string" ? game.storefront : "",
        smapi_version:
          typeof game.smapi_version === "string" ? game.smapi_version : null,
      },
      components,
      ...(collection ? { collection } : {}),
      ...(groups.length > 0 ? { groups } : {}),
    },
  };
}

export type DifferenceKind =
  | "missing"
  | "extra"
  | "version"
  | "package"
  | "enabled";

export interface Difference {
  kind: DifferenceKind;
  unique_id: string;
  /** Whether the recipe marks the component optional. */
  optional: boolean;
  /** Plain-language description of what differs. */
  detail: string;
  recipe?: RecipeComponent;
  installed?: ModListItemDto;
  /** The recipe marks the mod client-only, so the difference need not be
   * put right for multiplayer. */
  clientOnly?: boolean;
}

export interface RecipeComparison {
  matching: number;
  differences: Difference[];
  /** True when every listed component is identical. */
  identical: boolean;
  duplicates: string[];
}

/**
 * Compares the installed profile with a recipe by canonical UniqueID and exact
 * version. Versions are compared as text, not ranges: this is a reproduction
 * check, so "close enough" is reported as different.
 */
export function compareWithRecipe(
  installed: readonly ModListItemDto[],
  recipe: ProfileRecipe,
): RecipeComparison {
  const seen = new Set<string>();
  const duplicates = new Set<string>();
  const byId = new Map<string, ModListItemDto>();
  for (const mod of installed) {
    if (byId.has(mod.unique_id)) duplicates.add(mod.unique_id);
    byId.set(mod.unique_id, mod);
  }
  const differences: Difference[] = [];
  let matching = 0;

  for (const want of recipe.components) {
    if (seen.has(want.unique_id)) duplicates.add(want.unique_id);
    seen.add(want.unique_id);
    const have = byId.get(want.unique_id);
    if (!have) {
      differences.push({
        kind: "missing",
        unique_id: want.unique_id,
        optional: want.optional,
        detail: `Not installed. The recipe uses ${want.name} ${want.version}.`,
        recipe: want,
      });
      continue;
    }
    // A requirement that accepts newer versions is met by any version at
    // least as new; the package then does not have to match either.
    const newerIsFine =
      want.version_rule === "at_least" &&
      compareVersions(have.version, want.version) >= 0;
    if (have.version !== want.version && !newerIsFine) {
      differences.push({
        kind: "version",
        unique_id: want.unique_id,
        optional: want.optional,
        detail: `Installed ${have.version}, recipe uses ${want.version}.`,
        recipe: want,
        installed: have,
      });
      continue;
    }
    if (
      !newerIsFine &&
      want.artifact_hash &&
      have.artifact_hash &&
      want.artifact_hash !== have.artifact_hash
    ) {
      differences.push({
        kind: "package",
        unique_id: want.unique_id,
        optional: want.optional,
        detail: `Same version ${have.version} but a different package file than the recipe.`,
        recipe: want,
        installed: have,
      });
      continue;
    }
    if (have.enabled !== want.enabled) {
      differences.push({
        kind: "enabled",
        unique_id: want.unique_id,
        optional: want.optional,
        detail: `${have.enabled ? "Enabled" : "Disabled"} here, ${
          want.enabled ? "enabled" : "disabled"
        } in the recipe.`,
        recipe: want,
        installed: have,
      });
      continue;
    }
    matching += 1;
  }

  for (const have of installed) {
    if (!seen.has(have.unique_id)) {
      differences.push({
        kind: "extra",
        unique_id: have.unique_id,
        optional: false,
        detail: `Installed (${have.version}) but not in the recipe.`,
        installed: have,
      });
    }
  }

  for (const difference of differences) {
    if (difference.recipe?.client_only) difference.clientOnly = true;
  }
  differences.sort(
    (a, b) =>
      a.kind.localeCompare(b.kind) || a.unique_id.localeCompare(b.unique_id),
  );
  return {
    matching,
    differences,
    identical: differences.length === 0,
    duplicates: [...duplicates].sort(),
  };
}

const KIND_LABEL: Record<DifferenceKind, string> = {
  missing: "Missing",
  extra: "Extra",
  version: "Version differs",
  package: "Package differs",
  enabled: "Enabled state differs",
};

export function differenceLabel(kind: DifferenceKind): string {
  return KIND_LABEL[kind];
}

/** A key for remembering that a specific difference was accepted. */
export function differenceKey(difference: Difference): string {
  return `${difference.kind}:${difference.unique_id}:${
    difference.recipe?.version ?? ""
  }:${difference.installed?.version ?? ""}`;
}

export function renderComparison(
  comparison: RecipeComparison,
  accepted: ReadonlySet<string>,
): string {
  if (comparison.identical) {
    return "The installed profile matches the recipe.";
  }
  const lines = comparison.differences.map((d) => {
    const mark = accepted.has(differenceKey(d)) ? " (accepted)" : "";
    return `${differenceLabel(d.kind)}: ${d.unique_id} - ${d.detail}${mark}`;
  });
  return [
    `${comparison.matching} matching, ${comparison.differences.length} different`,
    ...lines,
  ].join("\n");
}
