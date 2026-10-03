import type { FindingDto, StoredCandidateDto } from "@/shared/api/generated";
import { missingRequirement, uniqueStoredFix } from "@/shared/health/fixPlan";

export interface RequirementPlan {
  /** Required mods with exactly one fitting stored package. */
  install: { uniqueId: string; candidate: StoredCandidateDto }[];
  /** Required mods with no fitting stored package, or several (the user's choice). */
  unresolved: string[];
}

/**
 * The required mods a newly installed mod is still missing, worked out from
 * the profile's health findings, and which of them a stored package can
 * supply without guessing.
 */
export async function requirementsFor(
  uniqueId: string,
  findings: readonly FindingDto[],
  find: (
    uniqueId: string,
    minimum: string | null,
  ) => Promise<StoredCandidateDto[]>,
): Promise<RequirementPlan> {
  const plan: RequirementPlan = { install: [], unresolved: [] };
  const id = uniqueId.toLowerCase();
  for (const finding of findings) {
    if (!finding.affected_entities.some((e) => e.toLowerCase() === id))
      continue;
    const missing = missingRequirement(finding);
    if (!missing) continue;
    const candidate = uniqueStoredFix(
      await find(missing.uniqueId, missing.minimum),
    );
    if (candidate) plan.install.push({ uniqueId: missing.uniqueId, candidate });
    else plan.unresolved.push(missing.uniqueId);
  }
  return plan;
}

/** Other mods from a pick-one choice that are already installed. */
export function chosenAlready(
  group: { name: string; choose: string } | undefined,
  members: readonly { unique_id: string; group?: string }[],
  installed: ReadonlySet<string>,
  except: string,
): string[] {
  if (!group || group.choose !== "one") return [];
  return members
    .filter(
      (m) =>
        m.group === group.name &&
        m.unique_id.toLowerCase() !== except.toLowerCase() &&
        installed.has(m.unique_id.toLowerCase()),
    )
    .map((m) => m.unique_id);
}
