import React from "react";
import { Card } from "@/components/ui/Card";
import type {
  DiagnosticsDto,
  DismissedFindingDto,
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import { HelpCircle } from "lucide-react";

export interface Unknown {
  what: string;
  why: string;
  next: string;
}

/**
 * Everything relevant to the active profile that the manager has not
 * established, gathered in one place. Unknowns are neither problems nor
 * passes; each says why it is unknown and what, if anything, can change it.
 */
export function listUnknowns(input: {
  overview: ProfileOverviewDto | undefined;
  report: DiagnosticsDto | undefined;
  mods: ModListItemDto[] | undefined;
  dismissed: DismissedFindingDto[] | undefined;
}): Unknown[] {
  const { overview, report, mods, dismissed } = input;
  const unknowns: Unknown[] = [
    {
      what: "Whether your mods are compatible with this game version",
      why: "No compatibility list is connected; only the minimum versions mods declare are checked.",
      next: "Nothing to do here. Mods' own pages are the best source.",
    },
    {
      what: "Whether newer versions of your mods exist",
      why: "No update source is connected. SMAPI's own update notices appear in Diagnostics when its log has them.",
      next: "Check the mods' pages yourself.",
    },
  ];
  const smapi = overview?.smapi_status;
  if (smapi?.is_installed && smapi.comparison === "unknown") {
    unknowns.push({
      what: "Which SMAPI version is installed",
      why: "Its version could not be read from the game folder.",
      next: "Repair or reinstall SMAPI from Settings if this persists.",
    });
  }
  for (const finding of overview?.health_summary.findings ?? []) {
    if (finding.code.endsWith("_UNASSESSED")) {
      unknowns.push({
        what: finding.title,
        why: finding.summary,
        next: "See the finding in Diagnostics for its evidence.",
      });
    }
  }
  const unknownSource = (mods ?? []).filter((m) => !m.artifact_hash);
  if (unknownSource.length > 0) {
    unknowns.push({
      what: `Where ${unknownSource.length} mod(s) came from`,
      why: "No stored package is recorded for them.",
      next: "Add your own source link in each mod's details, or install them again from a file.",
    });
  }
  unknowns.push({
    what: "Whether mod files were changed outside the manager",
    why: "Files are only compared when you ask, because it reads every file.",
    next: "Run Check mod files below.",
  });
  if (report && ["unknown", "unmatched"].includes(report.log_match)) {
    unknowns.push({
      what: "Which session the current SMAPI log belongs to",
      why:
        report.log_match === "unknown"
          ? "The log's start time could not be read."
          : "No game session is recorded for this profile.",
      next: "Start the game from the manager so the next log can be matched.",
    });
  }
  if ((dismissed ?? []).length > 0) {
    unknowns.push({
      what: `${dismissed?.length} finding(s) you dismissed`,
      why: "Dismissing hides a finding; it does not mean the manager checked it again.",
      next: "Show dismissed findings in Diagnostics to review them.",
    });
  }
  return unknowns;
}

export const UnknownsCard: React.FC<{
  overview: ProfileOverviewDto | undefined;
  report: DiagnosticsDto | undefined;
  mods: ModListItemDto[] | undefined;
  dismissed: DismissedFindingDto[] | undefined;
}> = (props) => {
  if (!props.overview) return null;
  const unknowns = listUnknowns(props);
  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <HelpCircle className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">What the manager does not know</h3>
      </div>
      <p className="text-xs text-[var(--fg-muted)]">
        These are not problems and not passes; they are things the manager has
        not established.
      </p>
      <ul className="text-xs space-y-2">
        {unknowns.map((unknown) => (
          <li key={unknown.what}>
            <p className="font-semibold">{unknown.what}</p>
            <p>{unknown.why}</p>
            <p className="text-[var(--fg-muted)]">{unknown.next}</p>
          </li>
        ))}
      </ul>
    </Card>
  );
};
