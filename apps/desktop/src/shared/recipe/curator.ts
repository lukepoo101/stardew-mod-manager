import type { FindingDto } from "@/shared/api/generated";
import type { OptionGroup, ProfileRecipe, RecipeComponent } from "./recipe";

/**
 * Checks a curator can run before sharing a recipe, and a changelog between two
 * revisions of one. Everything is derived from the recipe itself: nothing here
 * claims a mod is safe or that a recipient's install will work, only how much
 * of the recipe can be reproduced exactly.
 */

export type CheckStatus = "pass" | "warn" | "fail";

export interface CuratorCheck {
  id: string;
  label: string;
  status: CheckStatus;
  detail: string;
}

export interface CuratorReport {
  checks: CuratorCheck[];
  /** 0-100: how much of the recipe is pinned to exact evidence. */
  reproducibility: number;
  /** True when no check failed outright. */
  publishable: boolean;
}

const SHA256 = /^[0-9a-f]{64}$/i;

function names(components: readonly RecipeComponent[]): string {
  const list = components.slice(0, 5).map((c) => c.name || c.unique_id);
  const more = components.length - list.length;
  return more > 0 ? `${list.join(", ")} and ${more} more` : list.join(", ");
}

/** Health codes meaning a shared mod's requirement would not be met. */
const UNMET = new Set([
  "MISSING_DEPENDENCY",
  "DEPENDENCY_DISABLED",
  "DEPENDENCY_TOO_OLD",
]);

/**
 * `findings` are the profile's current health findings; when given, a mod
 * whose required mod is missing, disabled or too old fails the check, because
 * recipients would get the same broken requirement.
 */
export function checkRecipe(
  recipe: ProfileRecipe,
  findings?: readonly FindingDto[],
): CuratorReport {
  const checks: CuratorCheck[] = [];
  const components = recipe.components;

  if (components.length === 0) {
    checks.push({
      id: "empty",
      label: "Lists at least one mod",
      status: "fail",
      detail: "The recipe has no components, so there is nothing to share.",
    });
  }

  const seen = new Set<string>();
  const duplicated = new Set<string>();
  for (const component of components) {
    const key = component.unique_id.toLowerCase();
    if (seen.has(key)) duplicated.add(component.unique_id);
    seen.add(key);
  }
  checks.push(
    duplicated.size === 0
      ? {
          id: "duplicates",
          label: "Each mod is listed once",
          status: "pass",
          detail: "No UniqueID appears twice.",
        }
      : {
          id: "duplicates",
          label: "Each mod is listed once",
          status: "fail",
          detail: `Listed more than once: ${[...duplicated].join(", ")}.`,
        },
  );

  const unversioned = components.filter((c) => !c.version.trim());
  checks.push(
    unversioned.length === 0
      ? {
          id: "versions",
          label: "Every mod has an exact version",
          status: "pass",
          detail: "All versions are recorded.",
        }
      : {
          id: "versions",
          label: "Every mod has an exact version",
          status: "fail",
          detail: `No version for ${names(unversioned)}.`,
        },
  );

  const unpinned = components.filter((c) => !SHA256.test(c.artifact_hash));
  checks.push(
    unpinned.length === 0
      ? {
          id: "packages",
          label: "Every mod is pinned to a package checksum",
          status: "pass",
          detail: "Recipients can confirm they have the same files.",
        }
      : {
          id: "packages",
          label: "Every mod is pinned to a package checksum",
          status: "warn",
          detail: `No checksum for ${names(unpinned)}. A recipient cannot tell whether their copy matches.`,
        },
  );

  const enabled = components.filter((c) => c.enabled);
  checks.push(
    components.length > 0 && enabled.length === 0
      ? {
          id: "enabled",
          label: "At least one mod is enabled",
          status: "warn",
          detail:
            "Every mod is disabled, so a recipient would start with none running.",
        }
      : {
          id: "enabled",
          label: "At least one mod is enabled",
          status: "pass",
          detail: `${enabled.length} of ${components.length} enabled.`,
        },
  );

  checks.push(
    recipe.game.smapi_version
      ? {
          id: "smapi",
          label: "Records the SMAPI version",
          status: "pass",
          detail: `SMAPI ${recipe.game.smapi_version}.`,
        }
      : {
          id: "smapi",
          label: "Records the SMAPI version",
          status: "warn",
          detail: "Recipients cannot see which SMAPI this was built with.",
        },
  );

  const anonymous = components.filter(
    (c) => !c.name.trim() || !c.author.trim(),
  );
  checks.push(
    anonymous.length === 0
      ? {
          id: "attribution",
          label: "Names and authors are present",
          status: "pass",
          detail: "Every mod credits its author.",
        }
      : {
          id: "attribution",
          label: "Names and authors are present",
          status: "warn",
          detail: `Missing a name or author: ${names(anonymous)}.`,
        },
  );

  if (components.length > 0 && !components.some((c) => c.optional)) {
    checks.push({
      id: "optional",
      label: "Says which mods are optional",
      status: "warn",
      detail:
        "Nothing is marked optional. Recipients may assume every mod is required.",
    });
  }

  // Reproducibility is evidence, not quality: a version and a checksum per mod.
  const exact = components.filter(
    (c) => c.version.trim() && SHA256.test(c.artifact_hash),
  ).length;
  const reproducibility =
    components.length === 0 ? 0 : Math.round((exact / components.length) * 100);

  if (findings) {
    const unmet = findings.filter((f) => UNMET.has(f.code));
    checks.push(
      unmet.length === 0
        ? {
            id: "requirements",
            label: "Includes what its mods require",
            status: "pass",
            detail:
              "Every enabled mod's required mods are included, enabled and new enough.",
          }
        : {
            id: "requirements",
            label: "Includes what its mods require",
            status: "fail",
            detail: `Recipients would get the same unmet requirements: ${unmet
              .map((f) => f.title)
              .join("; ")}. Fix them on the Mods page first.`,
          },
    );
  }

  return {
    checks,
    reproducibility,
    publishable: !checks.some((check) => check.status === "fail"),
  };
}

/** Numeric-aware comparison of dotted versions; text parts compare as text. */
export function compareVersions(a: string, b: string): number {
  const split = (value: string) => value.split(/[.+-]/);
  const left = split(a);
  const right = split(b);
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const x = left[index] ?? "0";
    const y = right[index] ?? "0";
    const nx = Number(x);
    const ny = Number(y);
    const bothNumeric =
      x !== "" && y !== "" && !Number.isNaN(nx) && !Number.isNaN(ny);
    const order = bothNumeric ? nx - ny : x.localeCompare(y);
    if (order !== 0) return order < 0 ? -1 : 1;
  }
  return 0;
}

export interface Changelog {
  added: RecipeComponent[];
  removed: RecipeComponent[];
  upgraded: { from: RecipeComponent; to: RecipeComponent }[];
  downgraded: { from: RecipeComponent; to: RecipeComponent }[];
  /** Same version, different package file. */
  repackaged: { from: RecipeComponent; to: RecipeComponent }[];
  enabledChanged: { from: RecipeComponent; to: RecipeComponent }[];
  /**
   * Same mod and file, but the terms it is shared on changed: optional,
   * version rule, option group, manual source, client-only, author or the
   * curator's reason. Each entry says what changed, in words.
   */
  termsChanged: { to: RecipeComponent; changes: string[] }[];
  /** Option groups added, removed or redefined, in words. */
  groupChanges: string[];
  unchanged: number;
}

/** How a mod's sharing terms differ between two revisions, in words. */
export function termChanges(
  from: RecipeComponent,
  to: RecipeComponent,
): string[] {
  const changes: string[] = [];
  if (from.optional !== to.optional)
    changes.push(to.optional ? "now optional" : "now required");
  const rule = (c: RecipeComponent) => c.version_rule ?? "exact";
  if (rule(from) !== rule(to))
    changes.push(
      rule(to) === "at_least"
        ? "a newer version is now accepted"
        : "now needs this exact version",
    );
  if ((from.group ?? "") !== (to.group ?? ""))
    changes.push(
      to.group ? `now in the choice "${to.group}"` : "no longer in a choice",
    );
  if ((from.manual?.url ?? "") !== (to.manual?.url ?? ""))
    changes.push(
      to.manual
        ? `download it by hand from ${to.manual.url}`
        : "no longer fetched by hand",
    );
  else if (
    (from.manual?.instructions ?? "") !== (to.manual?.instructions ?? "")
  )
    changes.push("download instructions changed");
  if (Boolean(from.client_only) !== Boolean(to.client_only))
    changes.push(to.client_only ? "now client-only" : "no longer client-only");
  if (from.author !== to.author)
    changes.push(`author changed from "${from.author}" to "${to.author}"`);
  const sums = (c: RecipeComponent) =>
    (c.settings ?? [])
      .map((s) => `${s.path}:${s.sha256}`)
      .sort()
      .join(",");
  if (sums(from) !== sums(to))
    changes.push(
      !from.settings?.length
        ? "its settings are now shared"
        : !to.settings?.length
          ? "its settings are no longer shared"
          : "its shared settings changed",
    );
  if ((from.note ?? "") !== (to.note ?? ""))
    changes.push(to.note ? "the curator's reason changed" : "reason removed");
  return changes;
}

function groupChanges(
  before: readonly OptionGroup[],
  after: readonly OptionGroup[],
): string[] {
  const changes: string[] = [];
  const old = new Map(before.map((g) => [g.name, g]));
  for (const group of after) {
    const was = old.get(group.name);
    if (!was) changes.push(`New choice "${group.name}"`);
    else if (was.choose !== group.choose)
      changes.push(
        `"${group.name}" now ${group.choose === "one" ? "asks you to pick one" : "allows any number"}`,
      );
    else if (was.description !== group.description)
      changes.push(`"${group.name}" has a new description`);
  }
  const names = new Set(after.map((g) => g.name));
  for (const group of before)
    if (!names.has(group.name)) changes.push(`Choice "${group.name}" removed`);
  return changes;
}

export function diffRecipes(
  previous: ProfileRecipe,
  next: ProfileRecipe,
): Changelog {
  const before = new Map(previous.components.map((c) => [c.unique_id, c]));
  const after = new Map(next.components.map((c) => [c.unique_id, c]));
  const log: Changelog = {
    added: [],
    removed: [],
    upgraded: [],
    downgraded: [],
    repackaged: [],
    enabledChanged: [],
    termsChanged: [],
    groupChanges: groupChanges(previous.groups ?? [], next.groups ?? []),
    unchanged: 0,
  };
  for (const [id, to] of after) {
    const from = before.get(id);
    if (!from) {
      log.added.push(to);
      continue;
    }
    const order = compareVersions(from.version, to.version);
    if (order < 0) log.upgraded.push({ from, to });
    else if (order > 0) log.downgraded.push({ from, to });
    else if (
      from.artifact_hash &&
      to.artifact_hash &&
      from.artifact_hash.toLowerCase() !== to.artifact_hash.toLowerCase()
    ) {
      log.repackaged.push({ from, to });
    } else if (from.enabled !== to.enabled) {
      log.enabledChanged.push({ from, to });
    } else {
      const changes = termChanges(from, to);
      if (changes.length > 0) log.termsChanged.push({ to, changes });
      else log.unchanged += 1;
    }
  }
  for (const [id, from] of before) {
    if (!after.has(id)) log.removed.push(from);
  }
  const byId = (a: RecipeComponent, b: RecipeComponent) =>
    a.unique_id.localeCompare(b.unique_id);
  log.added.sort(byId);
  log.removed.sort(byId);
  for (const list of [
    log.upgraded,
    log.downgraded,
    log.repackaged,
    log.enabledChanged,
  ]) {
    list.sort((a, b) => byId(a.to, b.to));
  }
  return log;
}

export function isEmptyChangelog(log: Changelog): boolean {
  return (
    log.added.length +
      log.removed.length +
      log.upgraded.length +
      log.downgraded.length +
      log.repackaged.length +
      log.enabledChanged.length +
      log.termsChanged.length +
      log.groupChanges.length ===
    0
  );
}

/** Plain text a curator can paste into release notes. */
export function renderChangelog(
  previous: ProfileRecipe,
  next: ProfileRecipe,
  log: Changelog,
): string {
  if (isEmptyChangelog(log)) {
    return `No changes between "${previous.profile_name}" and "${next.profile_name}".`;
  }
  const lines: string[] = [
    `Changes from "${previous.profile_name}" to "${next.profile_name}"`,
  ];
  const section = (title: string, rows: string[]) => {
    if (rows.length > 0) lines.push("", `${title} (${rows.length})`, ...rows);
  };
  const label = (c: RecipeComponent) =>
    `${c.name || c.unique_id} (${c.unique_id})`;
  section(
    "Added",
    log.added.map((c) => `+ ${label(c)} ${c.version}`),
  );
  section(
    "Removed",
    log.removed.map((c) => `- ${label(c)} ${c.version}`),
  );
  section(
    "Updated",
    log.upgraded.map(
      (c) => `~ ${label(c.to)} ${c.from.version} to ${c.to.version}`,
    ),
  );
  section(
    "Rolled back",
    log.downgraded.map(
      (c) => `~ ${label(c.to)} ${c.from.version} to ${c.to.version}`,
    ),
  );
  section(
    "Same version, different package",
    log.repackaged.map((c) => `~ ${label(c.to)} ${c.to.version}`),
  );
  section(
    "Enabled state changed",
    log.enabledChanged.map(
      (c) => `~ ${label(c.to)} now ${c.to.enabled ? "enabled" : "disabled"}`,
    ),
  );
  section(
    "Sharing terms changed",
    log.termsChanged.map((c) => `~ ${label(c.to)}: ${c.changes.join("; ")}`),
  );
  section("Choices", log.groupChanges);
  lines.push("", `${log.unchanged} unchanged`);
  return lines.join("\n");
}
