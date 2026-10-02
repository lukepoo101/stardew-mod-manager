import React, { useMemo, useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import type {
  DiagnosticsDto,
  ModListItemDto,
  OperationDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import {
  buildSupportPlan,
  planWarnings,
  renderSupportBundle,
  renderSupportSummary,
  type SectionId,
} from "@/shared/support/export";
import { copyText, downloadText } from "@/shared/support/actions";
import { Download, LifeBuoy, ShieldCheck } from "lucide-react";

interface Props {
  report: DiagnosticsDto | undefined;
  overview: ProfileOverviewDto | undefined;
  mods: ModListItemDto[] | undefined;
  operations?: OperationDto[] | undefined;
  /** Fingerprints of dismissed findings, so the export says so. */
  acknowledged?: ReadonlySet<string>;
  managerVersion?: string | null;
}

/**
 * Builds a privacy-reviewed support export. The preview shows exactly the text
 * that is copied or downloaded, and nothing leaves the app without a click.
 */
export const SupportExportCard: React.FC<Props> = ({
  report,
  overview,
  mods,
  operations,
  acknowledged,
  managerVersion,
}) => {
  const [deselected, setDeselected] = useState<ReadonlySet<SectionId>>(
    new Set(),
  );
  const [status, setStatus] = useState<string | null>(null);
  const [reviewed, setReviewed] = useState(false);

  const plan = useMemo(
    () =>
      buildSupportPlan({
        report,
        overview,
        mods,
        operations,
        acknowledged,
        managerVersion,
        generatedAt: new Date().toISOString(),
      }),
    [report, overview, mods, operations, acknowledged, managerVersion],
  );
  const summary = renderSupportSummary(plan, deselected);
  const warnings = planWarnings(plan, deselected);
  // Missing evidence is informational; leftover sensitive-looking content is
  // not, so sharing waits for an explicit acknowledgement of that.
  const sensitiveWarnings = warnings.filter(
    (warning) => !warning.includes("evidence was unavailable"),
  );
  const shareBlocked = sensitiveWarnings.length > 0 && !reviewed;

  const toggle = (id: SectionId) => {
    const next = new Set(deselected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setDeselected(next);
  };

  const handleCopy = async () => {
    setStatus(
      (await copyText(summary))
        ? "Summary copied."
        : "Could not access the clipboard. Select the text above and copy it manually.",
    );
  };

  const handleDownload = () => {
    downloadText("support-bundle.json", renderSupportBundle(plan, deselected));
    setStatus("Bundle saved through your browser's download.");
  };

  return (
    <Card className="space-y-4">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <LifeBuoy className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Share for support</h3>
      </div>

      <p className="text-xs text-[var(--fg-muted)] leading-relaxed">
        Home-directory paths and secret-like values are replaced before this
        text is shown. Redaction is best effort, so read the preview before you
        share it. Nothing is uploaded.
      </p>

      <fieldset className="space-y-1.5">
        <legend className="text-xs font-semibold text-[var(--fg-muted)] uppercase tracking-wider mb-1">
          Included
        </legend>
        {plan.sections.map((section) => (
          <label
            key={section.id}
            className="flex items-start gap-2 text-xs cursor-pointer"
          >
            <input
              type="checkbox"
              className="mt-0.5"
              checked={!deselected.has(section.id)}
              onChange={() => toggle(section.id)}
            />
            <span>
              <span className="font-semibold">{section.title}</span>
              {section.unavailable ? " (unavailable)" : ""}
              <span className="block text-[var(--fg-muted)]">
                {section.reason}
                {section.essential &&
                  " Removing it makes the export much less useful."}
              </span>
            </span>
          </label>
        ))}
      </fieldset>

      <div
        className="flex items-center gap-1.5 text-xs text-[var(--fg-muted)]"
        aria-live="polite"
      >
        <ShieldCheck className="w-3.5 h-3.5" />
        <span>
          {plan.replacements.paths} path(s) and {plan.replacements.secrets}{" "}
          secret-like value(s) redacted.
        </span>
      </div>

      {warnings.length > 0 && (
        <ul
          className="text-xs space-y-1 p-3 rounded-lg border border-[var(--warning)]/30 bg-[var(--warning-surface)] text-[var(--warning)]"
          aria-label="Review before sharing"
        >
          {warnings.map((warning) => (
            <li key={warning}>{warning}</li>
          ))}
        </ul>
      )}

      {sensitiveWarnings.length > 0 && (
        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={reviewed}
            onChange={(event) => setReviewed(event.target.checked)}
          />
          <span>
            I have read the preview and accept sharing content flagged above.
          </span>
        </label>
      )}

      <textarea
        readOnly
        aria-label="Support summary preview"
        rows={12}
        value={summary}
        className="w-full p-4 rounded-xl bg-[var(--bg-primary)] border border-[var(--border)] text-xs font-mono text-[var(--fg-muted)] leading-relaxed"
      />

      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          variant="secondary"
          onClick={handleCopy}
          disabled={shareBlocked}
        >
          Copy summary
        </Button>
        <Button
          size="sm"
          variant="secondary"
          onClick={handleDownload}
          disabled={shareBlocked}
          className="flex items-center gap-1.5"
        >
          <Download className="w-3.5 h-3.5" />
          <span>Save bundle</span>
        </Button>
        {status && (
          <span role="status" className="text-xs text-[var(--fg-muted)]">
            {status}
          </span>
        )}
      </div>
    </Card>
  );
};
