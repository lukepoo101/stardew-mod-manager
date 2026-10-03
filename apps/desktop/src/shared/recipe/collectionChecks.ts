import type {
  DependencyMapEntryDto,
  FindingDto,
  ShareableSettingsDto,
} from "@/shared/api/generated";
import type { CollectionDraft } from "./collection";
import { checkRecipe, type Changelog, isEmptyChangelog } from "./curator";
import type { ProfileRecipe, RecipeComponent } from "./recipe";
import type { Redaction } from "./settingsPrivacy";

function modNames(components: readonly { name: string; unique_id: string }[]) {
  return components.map((c) => c.name || c.unique_id).join(", ");
}

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
    /** The newest published revision, when there is one. */
    latestRevision?: number;
    /** Mods with settings files, with what each might reveal. */
    shareable?: readonly ShareableSettingsDto[];
    /** Settings files left out when read, with why. */
    skippedSettings?: readonly string[];
    /** What redaction did to each shared settings file, by mod. */
    settingsReports?: ReadonlyMap<
      string,
      readonly { path: string; report: Redaction }[]
    >;
    /** Lower-case UniqueIDs of mods whose files differ from their package
     * here (changed outside the manager, or such changes accepted). */
    locallyModified?: ReadonlySet<string>;
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
    // Needed only by optional mods: recipients who decline those do not
    // need it either, so it should be optional too.
    else if (needers && needers.length > 0 && !component.optional) {
      // The map names dependents by display name.
      const byName = new Map(
        recipe.components.map((c) => [c.name.toLowerCase(), c]),
      );
      const needing = needers.map((name) => byName.get(name.toLowerCase()));
      if (needing.every((c) => c?.optional))
        checks.push({
          level: "warning",
          subject: component.unique_id,
          message: `${component.name} is required, but only optional mods need it (${needers.join(", ")}). Mark it optional so recipients who decline them do not get it.`,
        });
    }
  }
  if (checks.some((c) => /no mod in the collection needs it/.test(c.message)))
    checks.push({
      level: "limitation",
      message:
        "Unused requirements are worked out from manifest dependencies only. A mod can rely on another without declaring it, so treat these as hints, not proof.",
    });

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

  // Mods whose files here are not what their package installs: recipients
  // get the package, not these files.
  for (const component of recipe.components) {
    if (options.locallyModified?.has(component.unique_id.toLowerCase()))
      checks.push({
        level: "warning",
        subject: component.unique_id,
        message: `${component.name}'s files here differ from its package (changed outside the manager). Recipients get the package as published, not your changes.`,
      });
  }

  // Shared settings: what might be private, and what could not be shared.
  for (const component of recipe.components) {
    const choice = draft.mods[component.unique_id.toLowerCase()];
    if (!choice?.includeSettings) continue;
    const reports = options.settingsReports?.get(
      component.unique_id.toLowerCase(),
    );
    for (const { path, report } of reports ?? []) {
      const where = `${component.name}'s ${path}`;
      if (!report.checked)
        checks.push({
          level: "warning",
          subject: component.unique_id,
          message: `${where} is not JSON, so it could not be checked field by field. Leave its settings out unless you know what is in it.`,
        });
      if (report.blocked.length > 0)
        checks.push({
          level: "limitation",
          message: `Never shared from ${where}: ${report.blocked.join(", ")} (keys, tokens or passwords).`,
        });
      if (report.removed.length > 0)
        checks.push({
          level: "limitation",
          message: `Left out of ${where}: ${report.removed.join(", ")}. Tick "Share flagged fields too" to include them.`,
        });
      if (report.remaining.length > 0)
        checks.push({
          level: "warning",
          subject: component.unique_id,
          message: `${where} still includes fields that may be private: ${report.remaining.map((f) => `${f.path} (${f.kind})`).join(", ")}.`,
        });
    }
    const found = reports
      ? undefined
      : options.shareable?.find(
          (s) =>
            s.unique_id.toLowerCase() === component.unique_id.toLowerCase(),
        );
    if (found && found.warnings.length > 0)
      checks.push({
        level: "warning",
        subject: component.unique_id,
        message: `${component.name}'s settings may include private details: ${found.warnings.join("; ")}. Leave them out, or publish knowing they are shared.`,
      });
    if (!component.settings?.length)
      checks.push({
        level: "warning",
        subject: component.unique_id,
        message: `${component.name}'s settings were chosen, but it has none that can be shared.`,
      });
  }
  for (const skipped of options.skippedSettings ?? [])
    checks.push({
      level: "limitation",
      message: `Not shared: ${skipped}. Only text config.json files up to 256 KB can be carried.`,
    });
  const withSettings = recipe.components.filter((c) => c.settings?.length);
  if (withSettings.length > 0)
    checks.push({
      level: "limitation",
      message: `Settings are shared for ${modNames(withSettings)}. Recipients choose whether to use them, and their own are backed up first.`,
    });

  // Clean-profile testing: what has and has not been tried, with when.
  if (options.latestRevision !== undefined) {
    const test = draft.cleanTest;
    checks.push({
      level: "limitation",
      message: !test
        ? "No published revision has been tried in a clean profile yet."
        : test.revision < options.latestRevision
          ? `The last clean-profile test was of revision ${test.revision} (${new Date(test.at).toLocaleString()}); revision ${options.latestRevision} has not been tried.`
          : `Revision ${test.revision} was set up in the clean profile "${test.profileName}" on ${new Date(test.at).toLocaleString()}. Its health there is the result.`,
    });
  }

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

export interface RecipientSimulation {
  /** Required mods a recipient gets by default. */
  required: number;
  /** Required mods with a manual download link from the curator. */
  manualLinks: string[];
  /** Required mods identified only by where the author publishes them. */
  publishedAt: string[];
  /** Required mods with neither: the recipient must find them. */
  unlocated: string[];
  /** Mods whose exact file cannot be confirmed (no checksum). */
  unverifiable: string[];
  /** Mods that bring shared settings. */
  withSettings: string[];
  /** Each option group, simulated separately. */
  choices: { name: string; pickOne: boolean; options: string[] }[];
  /** Optional mods outside any group. */
  optionalAlone: string[];
  /** What the simulation assumes, so it is not read as more than it is. */
  assumptions: string[];
}

/**
 * What someone with none of these packages meets when following the
 * collection, worked out from the recipe alone. Nothing online is checked.
 */
export function simulateRecipient(recipe: ProfileRecipe): RecipientSimulation {
  const name = (c: RecipeComponent) => c.name || c.unique_id;
  const required = recipe.components.filter((c) => !c.optional);
  return {
    required: required.length,
    manualLinks: required.filter((c) => c.manual).map(name),
    publishedAt: required
      .filter((c) => !c.manual && (c.update_keys?.length ?? 0) > 0)
      .map((c) => `${name(c)} (${c.update_keys?.join(", ")})`),
    unlocated: required
      .filter(
        (c) => !c.manual && !(c.update_keys?.length ?? 0) && !c.source_url,
      )
      .map(name),
    unverifiable: recipe.components.filter((c) => !c.artifact_hash).map(name),
    withSettings: recipe.components.filter((c) => c.settings?.length).map(name),
    choices: (recipe.groups ?? []).map((g) => ({
      name: g.name,
      pickOne: g.choose === "one",
      options: recipe.components.filter((c) => c.group === g.name).map(name),
    })),
    optionalAlone: recipe.components
      .filter((c) => c.optional && !c.group)
      .map(name),
    assumptions: [
      "A recipient with none of these packages stored, and no bundle.",
      "Nothing online is checked, so availability and sizes are unknown.",
      "No mod site accounts are involved: every download is done by the recipient.",
    ],
  };
}
