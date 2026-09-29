import type {
  DiagnosticsDto,
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import { operatingSystemLabel } from "@/shared/platform/labels";
import {
  mergeCounts,
  redactText,
  residualWarnings,
  type RedactionResult,
} from "./redact";

export const SUPPORT_BUNDLE_SCHEMA = "stardew-mod-manager.support-bundle";
export const SUPPORT_BUNDLE_SCHEMA_VERSION = 1;

/** Lines of the SMAPI log kept in an export; a full log is never the default. */
export const LOG_TAIL_LINES = 60;

export type SectionId = "environment" | "profile" | "mods" | "findings" | "log";

export interface SupportSection {
  id: SectionId;
  title: string;
  /** Why the section is included, shown before anything is shared. */
  reason: string;
  /** Whether leaving it out makes the export much less useful. */
  essential: boolean;
  /** Redacted text of the section. */
  text: string;
  /** True when the underlying evidence was unavailable, not merely empty. */
  unavailable: boolean;
}

export interface SupportInput {
  report: DiagnosticsDto | undefined;
  overview: ProfileOverviewDto | undefined;
  mods: ModListItemDto[] | undefined;
  managerVersion?: string | null;
  generatedAt: string;
}

export interface SupportPlan {
  generatedAt: string;
  sections: SupportSection[];
  replacements: RedactionResult["replacements"];
}

function tail(log: string, lines: number): string {
  const all = log.split(/\r?\n/);
  return all.slice(-lines).join("\n");
}

function normaliseSeverity(severity: string): string {
  return severity.trim().toLowerCase();
}

/**
 * Builds the export plan. Every section is redacted here, once, so the
 * clipboard summary, the preview and the written bundle are all derived from
 * the same already-sanitised text and cannot drift apart.
 */
export function buildSupportPlan(input: SupportInput): SupportPlan {
  const { report, overview, mods } = input;
  let replacements = { paths: 0, secrets: 0 };
  const redact = (raw: string): string => {
    const result = redactText(raw);
    replacements = mergeCounts(replacements, result.replacements);
    return result.text;
  };

  const sections: SupportSection[] = [];

  const environment = [
    `Manager version: ${input.managerVersion ?? "unknown"}`,
    `Platform: ${
      report ? operatingSystemLabel(report.host_operating_system) : "unknown"
    }`,
    `Game: ${
      overview
        ? `${overview.game.storefront} (${overview.game.operating_system})`
        : "unknown"
    }`,
    `SMAPI: ${
      overview
        ? overview.smapi_status.is_installed
          ? `${overview.smapi_status.observed_version ?? "version not detected"}${
              overview.smapi_status.is_compatible
                ? ""
                : " (not the tested version)"
            }`
          : "not installed"
        : "unknown"
    }`,
  ].join("\n");
  sections.push({
    id: "environment",
    title: "Environment",
    reason: "Manager, platform, game and SMAPI versions narrow most problems.",
    essential: true,
    text: redact(environment),
    unavailable: !report || !overview,
  });

  sections.push({
    id: "profile",
    title: "Profile",
    reason: "The profile name and revision identify the state being described.",
    essential: false,
    text: redact(
      overview
        ? `Profile: ${overview.profile.name} (revision ${overview.profile.revision})\nMods: ${overview.mod_count}`
        : "Profile: unavailable",
    ),
    unavailable: !overview,
  });

  const modLines = (mods ?? [])
    .slice()
    .sort((a, b) => a.unique_id.localeCompare(b.unique_id))
    .map(
      (mod) =>
        `${mod.unique_id} ${mod.version} ${mod.enabled ? "enabled" : "disabled"}`,
    );
  sections.push({
    id: "mods",
    title: "Installed mods",
    reason: "Exact UniqueIDs and versions let a helper reproduce the setup.",
    essential: true,
    text: redact(
      mods ? modLines.join("\n") || "No mods installed" : "Unavailable",
    ),
    unavailable: !mods,
  });

  const findingLines = (report?.findings ?? []).map(
    (finding) =>
      `[${normaliseSeverity(finding.severity)}] ${finding.code}: ${finding.summary}`,
  );
  sections.push({
    id: "findings",
    title: "Health findings",
    reason: "Findings show what the manager itself noticed.",
    essential: true,
    text: redact(
      report
        ? findingLines.join("\n") ||
            "No findings were reported. This is not proof the setup is healthy."
        : "Unavailable",
    ),
    unavailable: !report,
  });

  const logAvailable = Boolean(report?.raw_log?.trim());
  sections.push({
    id: "log",
    title: `SMAPI log (last ${LOG_TAIL_LINES} lines)`,
    reason:
      "Recent log lines often contain the actual error. Logs can contain personal information, so review them.",
    essential: false,
    text: redact(
      logAvailable
        ? tail(report?.raw_log ?? "", LOG_TAIL_LINES)
        : "No SMAPI log was available.",
    ),
    unavailable: !logAvailable,
  });

  return { generatedAt: input.generatedAt, sections, replacements };
}

export function selectedSections(
  plan: SupportPlan,
  deselected: ReadonlySet<SectionId>,
): SupportSection[] {
  return plan.sections.filter((section) => !deselected.has(section.id));
}

/** Concise, paste-friendly text. */
export function renderSupportSummary(
  plan: SupportPlan,
  deselected: ReadonlySet<SectionId> = new Set(),
): string {
  const parts = selectedSections(plan, deselected).map((section) => {
    const note = section.unavailable ? " (unavailable)" : "";
    return `## ${section.title}${note}\n${section.text}`;
  });
  return [`Support summary - ${plan.generatedAt}`, ...parts].join("\n\n");
}

/** Everything that still needs a human eye before this text is shared. */
export function planWarnings(
  plan: SupportPlan,
  deselected: ReadonlySet<SectionId> = new Set(),
): string[] {
  const warnings: string[] = [];
  for (const section of selectedSections(plan, deselected)) {
    for (const warning of residualWarnings(section.text)) {
      warnings.push(`${section.title}: ${warning}`);
    }
    if (section.unavailable) {
      warnings.push(
        `${section.title}: evidence was unavailable and is marked as such, not reported as healthy.`,
      );
    }
  }
  return warnings;
}

export function renderSupportBundle(
  plan: SupportPlan,
  deselected: ReadonlySet<SectionId> = new Set(),
): string {
  const included = selectedSections(plan, deselected);
  return `${JSON.stringify(
    {
      schema: SUPPORT_BUNDLE_SCHEMA,
      schema_version: SUPPORT_BUNDLE_SCHEMA_VERSION,
      generated_at: plan.generatedAt,
      redaction: {
        applied: true,
        paths_redacted: plan.replacements.paths,
        secrets_redacted: plan.replacements.secrets,
        note: "Redaction is best effort; review before sharing.",
      },
      sections: included.map((section) => ({
        id: section.id,
        title: section.title,
        unavailable: section.unavailable,
        text: section.text,
      })),
    },
    null,
    2,
  )}\n`;
}
