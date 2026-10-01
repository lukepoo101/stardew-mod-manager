import type { FindingDto } from "@/shared/api/generated";

/**
 * What each finding means for the user and what to do about it, keyed by the
 * finding's stable code. Copy lives here, next to the UI, while the backend
 * owns the facts. A code without an entry still gets an honest answer:
 * "investigate manually", never an invented action.
 */

export type EvidenceSource =
  | "Manager records"
  | "SMAPI log"
  | "Game folder"
  | "Mod manifests"
  | "Runtime observation"
  | "Manager check";

export interface FindingGuidance {
  /** Why it matters, in plain words. */
  impact: string;
  /** The canonical place to act, or null when it needs investigation by hand. */
  action: { label: string; to: string } | null;
  source: EvidenceSource;
  /**
   * Whether the finding states something observed directly, or something the
   * manager inferred from observations (such as "mods may need updates").
   */
  certainty: "observed" | "inferred";
}

const CATALOGUE: Record<string, FindingGuidance> = {
  RECOVERY_REQUIRED: {
    impact:
      "A change was interrupted, so the profile's files may not match what the manager recorded. Launching now could load a half-applied change.",
    action: { label: "Review in Activity", to: "/app/activity" },
    source: "Manager records",
    certainty: "observed",
  },
  SMAPI_MISSING: {
    impact: "Without SMAPI, Stardew Valley starts without any mods.",
    action: { label: "Set up SMAPI", to: "/app/overview" },
    source: "Game folder",
    certainty: "observed",
  },
  MISSING_DEPENDENCY: {
    impact:
      "A mod that needs another mod will not load until that mod is installed and enabled.",
    action: { label: "Open Mods", to: "/app/mods" },
    source: "Mod manifests",
    certainty: "observed",
  },
  DEPENDENCY_DISABLED: {
    impact:
      "SMAPI skips a mod whose required mod is turned off, so the dependent mod will not load.",
    action: { label: "Open Mods", to: "/app/mods" },
    source: "Mod manifests",
    certainty: "observed",
  },
  DEPENDENCY_TOO_OLD: {
    impact:
      "SMAPI refuses to load a mod when the mod it depends on is older than the version it asks for.",
    action: { label: "Open Mods", to: "/app/mods" },
    source: "Mod manifests",
    certainty: "observed",
  },
  DEPENDENCY_UNASSESSED: {
    impact:
      "The required version could not be compared, so this requirement is unchecked rather than known to be met.",
    action: { label: "Open Mods", to: "/app/mods" },
    source: "Mod manifests",
    certainty: "inferred",
  },
  DUPLICATE_UNIQUE_ID: {
    impact:
      "SMAPI loads only one copy of a mod ID and skips the others, so you cannot be sure which version runs.",
    action: { label: "Open Mods", to: "/app/mods" },
    source: "Mod manifests",
    certainty: "observed",
  },
  REFERENCE_MODS_MISSING: {
    impact:
      "Playing with the group may fail or behave differently while mods the group requires are missing.",
    action: { label: "Open Profiles", to: "/app/profiles" },
    source: "Mod manifests",
    certainty: "observed",
  },
  RUNTIME_GAME_CHANGED: {
    impact:
      "Mods written for the earlier version may break or be skipped. This is a possibility, not a detected failure.",
    action: { label: "Check the last session", to: "/app/diagnostics" },
    source: "Runtime observation",
    certainty: "inferred",
  },
  RUNTIME_SMAPI_CHANGED: {
    impact:
      "Mods written for the earlier SMAPI may break or be skipped. This is a possibility, not a detected failure.",
    action: { label: "Check the last session", to: "/app/diagnostics" },
    source: "Runtime observation",
    certainty: "inferred",
  },
  VERIFICATION_UNAVAILABLE: {
    impact:
      "The manager could not confirm which mods loaded, so problems in the last session may be hidden.",
    action: null,
    source: "SMAPI log",
    certainty: "observed",
  },
  LOG_MOD_SKIPPED: {
    impact:
      "SMAPI did not load these mods, so their features are missing in game.",
    action: { label: "Open Mods", to: "/app/mods" },
    source: "SMAPI log",
    certainty: "observed",
  },
  LOG_MOD_ERRORS: {
    impact:
      "Errors from a mod often mean a feature is broken or an incompatibility. The log lines say where.",
    action: { label: "Find the mod causing it", to: "/app/diagnostics" },
    source: "SMAPI log",
    certainty: "observed",
  },
  LOG_UPDATES_REPORTED: {
    impact:
      "Newer versions exist. Updating is optional; nothing is broken because of this.",
    action: { label: "Open Mods", to: "/app/mods" },
    source: "SMAPI log",
    certainty: "observed",
  },
  LOG_ERROR_DETECTED: {
    impact: "SMAPI logged errors. Some may be harmless; the lines show which.",
    action: null,
    source: "SMAPI log",
    certainty: "observed",
  },
  MOD_NEEDS_NEWER_SMAPI: {
    impact:
      "SMAPI will not load these mods until it is at least the version they name.",
    action: { label: "Update SMAPI", to: "/app/overview" },
    source: "Mod manifests",
    certainty: "observed",
  },
  MOD_NEEDS_NEWER_SMAPI_UNASSESSED: {
    impact:
      "The minimum SMAPI version could not be compared, so these mods were not checked.",
    action: null,
    source: "Mod manifests",
    certainty: "observed",
  },
  MOD_NEEDS_NEWER_GAME: {
    impact:
      "These mods declare they need a newer Stardew Valley and may refuse to load.",
    action: null,
    source: "Mod manifests",
    certainty: "observed",
  },
  MOD_NEEDS_NEWER_GAME_UNASSESSED: {
    impact:
      "The game's version or a mod's minimum could not be read, so these mods were not checked.",
    action: null,
    source: "Mod manifests",
    certainty: "observed",
  },
  SMAPI_LOG_MISSING: {
    impact:
      "Without a log the manager cannot tell what happened in the last session.",
    action: { label: "Launch the game once", to: "/app/overview" },
    source: "Game folder",
    certainty: "observed",
  },
};

const UNKNOWN: FindingGuidance = {
  impact: "",
  action: null,
  source: "Manager check",
  certainty: "observed",
};

export function guidanceFor(
  finding: Pick<FindingDto, "code">,
): FindingGuidance {
  return CATALOGUE[finding.code] ?? UNKNOWN;
}

export function knownFindingCodes(): string[] {
  return Object.keys(CATALOGUE);
}
