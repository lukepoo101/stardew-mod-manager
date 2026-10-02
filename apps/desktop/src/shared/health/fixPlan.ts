import type { FindingDto, ModListItemDto } from "@/shared/api/generated";

/** A change the manager can make with certainty, and why. */
export interface PlannedFix {
  /** The installed, disabled mod to enable. */
  mod: ModListItemDto;
  /** The finding that justifies it. */
  because: FindingDto;
}

/** A finding left alone, with the reason it is not fixed automatically. */
export interface LeftAlone {
  finding: FindingDto;
  reason: string;
}

const NOT_AUTOMATIC: Record<string, string> = {
  MISSING_DEPENDENCY:
    "The required mod is not installed here; install it from a file you trust.",
  DEPENDENCY_TOO_OLD:
    "A newer version is needed; get it and install it so the change can be reviewed.",
  DUPLICATE_UNIQUE_ID:
    "Which copy to keep is your choice; disable or remove the others.",
  MOD_NEEDS_NEWER_SMAPI: "SMAPI needs updating, which is done on its own.",
  MOD_NEEDS_NEWER_GAME: "The game needs updating outside the manager.",
};

const quotedId = (text: string) => text.match(/'([^']+)'/)?.[1];

/** The UniqueID and minimum version a missing-dependency finding names. */
export function missingRequirement(
  finding: FindingDto,
): { uniqueId: string; minimum: string | null } | null {
  if (finding.code !== "MISSING_DEPENDENCY") return null;
  const uniqueId = quotedId(finding.title);
  if (!uniqueId) return null;
  const escaped = uniqueId.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const minimum =
    finding.summary.match(new RegExp(`'${escaped}' ([^ ]+) or newer`))?.[1] ??
    null;
  return { uniqueId, minimum };
}

/**
 * The one stored package to install for a missing requirement, or null when
 * there is none or more than one would do (that choice is the user's).
 */
export function uniqueStoredFix<
  T extends { meets_minimum: boolean | null; version: string },
>(candidates: readonly T[]): T | null {
  const fitting = candidates.filter((c) => c.meets_minimum === true);
  return fitting.length === 1 ? fitting[0] : null;
}

/**
 * Works out which health findings have a single, certain fix: a required mod
 * that is installed but disabled is enabled. Everything else that could stop
 * a mod loading is listed as left alone, with why.
 */
export function planFixes(
  findings: readonly FindingDto[],
  mods: readonly ModListItemDto[],
): { fixes: PlannedFix[]; leftAlone: LeftAlone[] } {
  const fixes: PlannedFix[] = [];
  const leftAlone: LeftAlone[] = [];
  const planned = new Set<string>();
  for (const finding of findings) {
    if (finding.code === "DEPENDENCY_DISABLED") {
      const id = quotedId(finding.title)?.toLowerCase();
      const disabled = mods.filter(
        (m) => m.unique_id.toLowerCase() === id && !m.enabled,
      );
      if (disabled.length === 1) {
        if (!planned.has(disabled[0].profile_component_id)) {
          planned.add(disabled[0].profile_component_id);
          fixes.push({ mod: disabled[0], because: finding });
        }
      } else {
        leftAlone.push({
          finding,
          reason:
            disabled.length > 1
              ? "More than one disabled copy is installed; choose which to enable."
              : "The disabled copy could not be found in the list.",
        });
      }
    } else if (NOT_AUTOMATIC[finding.code]) {
      leftAlone.push({ finding, reason: NOT_AUTOMATIC[finding.code] });
    }
  }
  return { fixes, leftAlone };
}
