import type { DependencyMapEntryDto, FindingDto } from "@/shared/api/generated";
import type { CollectionDraft } from "./collection";
import { checkRecipe, type Changelog, isEmptyChangelog } from "./curator";
import type { ProfileRecipe } from "./recipe";

export type CheckLevel = "error" | "warning" | "limitation";

export interface CollectionCheck {
  level: CheckLevel;
  message: string;
  /** The mod or group it is about, when there is one. */
  subject?: string;
}

export interface CollectionChecks {
  checks: CollectionCheck[];
  /** True only when a recipient needs no manual steps at all. */
  fullyAutomatic: boolean;
  /** What a new recipient will meet, by kind. */
  recipient: {
    exactFiles: number;
    manual: number;
    optional: number;
    noChecksum: number;
  };
  /** 0-100, from the recipe check: share of mods pinned to an exact file. */
  reproducibility: number;
}

/**
 * Everything worth knowing before a collection revision is published, split
 * into errors (publishing is blocked), warnings (publish only deliberately)
 * and limitations (facts recipients will meet). Read-only; can run on every
 * edit of the draft.
 */
export function checkCollection(
  recipe: ProfileRecipe,
  draft: CollectionDraft,
  options: {
    findings?: readonly FindingDto[];
    map?: readonly DependencyMapEntryDto[];
    /** Lower-case UniqueIDs of mods installed as another mod's requirement. */
    installedAsDependency?: ReadonlySet<string>;
    changelog?: Changelog | null;
  } = {},
): CollectionChecks {
  const checks: CollectionCheck[] = [];
  const base = checkRecipe(recipe, options.findings);
  for (const check of base.checks) {
    if (check.status === "fail")
      checks.push({
        level: "error",
        message: `${check.label}: ${check.detail}`,
      });
    else if (check.status === "warn")
      checks.push({
        level: "warning",
        message: `${check.label}: ${check.detail}`,
      });
  }

  if (!draft.name.trim())
    checks.push({ level: "error", message: "The collection needs a name." });
  if (!draft.author.trim())
    checks.push({
      level: "warning",
      message:
        "No author is given, so recipients cannot tell who publishes it.",
    });

  // Option groups: each must be used, described, and a choose-one group
  // needs at least two members to be a choice.
  for (const group of draft.groups) {
    const members = recipe.components.filter((c) => c.group === group.name);
    if (members.length === 0)
      checks.push({
        level: "warning",
        subject: group.name,
        message: `The group "${group.name}" has no mods and will be left out.`,
      });
    else if (group.choose === "one" && members.length < 2)
      checks.push({
        level: "error",
        subject: group.name,
        message: `"${group.name}" asks recipients to choose one, but only one mod is in it.`,
      });
    if (members.length > 0 && !group.description.trim())
      checks.push({
        level: "warning",
        subject: group.name,
        message: `"${group.name}" has no description, so recipients will not know what they are choosing.`,
      });
  }
  const names = new Set(draft.groups.map((g) => g.name));
  if (names.size !== draft.groups.length)
    checks.push({
      level: "error",
      message: "Two option groups have the same name.",
    });

  // Dependency-only mods nothing needs any more.
  const neededBy = new Map(
    (options.map ?? []).map((e) => [e.unique_id.toLowerCase(), e.required_by]),
  );
  for (const component of recipe.components) {
    const choice = draft.mods[component.unique_id.toLowerCase()];
    const needers = neededBy.get(component.unique_id.toLowerCase());
    if (
      needers !== undefined &&
      needers.length === 0 &&
      choice?.intended !== true &&
      options.installedAsDependency?.has(component.unique_id.toLowerCase())
    )
      checks.push({
        level: "warning",
        subject: component.unique_id,
        message: `${component.name} was installed as a requirement, but no mod in the collection needs it now. Remove it, or mark it as intended.`,
      });
  }

  if (
    options.changelog &&
    !isEmptyChangelog(options.changelog) &&
    !draft.notes.trim()
  )
    checks.push({
      level: "warning",
      message:
        "Mods changed since the last revision, but there are no notes saying why.",
    });

  const manual = recipe.components.filter((c) => c.manual);
  const optional = recipe.components.filter((c) => c.optional);
  const noChecksum = recipe.components.filter((c) => !c.artifact_hash);
  if (manual.length > 0)
    checks.push({
      level: "limitation",
      message: `${manual.length} mod(s) must be downloaded by hand from the links given.`,
    });
  if (noChecksum.length > 0)
    checks.push({
      level: "limitation",
      message: `${noChecksum.length} mod(s) have no recorded file checksum, so recipients cannot be sure they get the same file.`,
    });

  return {
    checks,
    fullyAutomatic: manual.length === 0 && noChecksum.length === 0,
    recipient: {
      exactFiles: recipe.components.length - noChecksum.length,
      manual: manual.length,
      optional: optional.length,
      noChecksum: noChecksum.length,
    },
    reproducibility: base.reproducibility,
  };
}
