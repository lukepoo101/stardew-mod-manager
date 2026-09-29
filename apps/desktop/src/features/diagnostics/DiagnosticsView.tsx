import React, { useMemo, useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useBootstrap,
  useDiagnosticsReport,
  useActiveProfileOverview,
  useProfileMods,
} from "@/shared/api/hooks";
import { redactText } from "@/shared/support/redact";
import { copyText } from "@/shared/support/actions";
import {
  EMPTY_FILTER,
  filterFindings,
  findingCounts,
  severityKey,
  type SeverityKey,
} from "@/shared/support/findings";
import { SupportExportCard } from "./SupportExportCard";
import { operatingSystemLabel } from "@/shared/platform/labels";
import {
  AlertTriangle,
  CheckCircle2,
  Copy,
  HardDrive,
  MonitorCog,
  RefreshCw,
  Terminal,
} from "lucide-react";

export const DiagnosticsView: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const gameId = overview?.game.id;
  const { data: report, isLoading, refetch } = useDiagnosticsReport(gameId);
  const { data: bootstrap } = useBootstrap();
  const { data: mods } = useProfileMods(overview?.profile.id);

  const [copied, setCopied] = useState<"redacted" | "raw" | "failed" | null>(
    null,
  );
  const [severityFilter, setSeverityFilter] = useState<
    ReadonlySet<SeverityKey>
  >(new Set());
  const [categoryFilter, setCategoryFilter] = useState<ReadonlySet<string>>(
    new Set(),
  );
  const [logQuery, setLogQuery] = useState("");
  const logLines = useMemo(
    () => (report?.raw_log ?? "").split(/\r?\n/),
    [report?.raw_log],
  );
  const matchingLines = useMemo(() => {
    const query = logQuery.trim().toLowerCase();
    if (!query) return null;
    return logLines
      .map((text, index) => ({ text, number: index + 1 }))
      .filter((line) => line.text.toLowerCase().includes(query));
  }, [logLines, logQuery]);
  const findings = report?.findings ?? [];
  const counts = findingCounts(findings);
  const visibleFindings = filterFindings(findings, {
    ...EMPTY_FILTER,
    severities: severityFilter,
    categories: categoryFilter,
  });
  const toggleIn = <T,>(set: ReadonlySet<T>, value: T): Set<T> => {
    const next = new Set(set);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    return next;
  };
  const [refreshing, setRefreshing] = useState(false);

  // Copied logs get the same redaction as the support export by default; the
  // unredacted original is available only through an explicit second action.
  const handleCopyLog = async (redacted: boolean) => {
    if (!report?.raw_log) return;
    const text = redacted ? redactText(report.raw_log).text : report.raw_log;
    setCopied(
      (await copyText(text)) ? (redacted ? "redacted" : "raw") : "failed",
    );
    setTimeout(() => setCopied(null), 2500);
  };

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      await refetch();
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-bold tracking-tight">
            Diagnostics & Logs
          </h2>
          <p className="text-sm text-[var(--fg-muted)]">
            Inspect compatibility findings, launch session history, and live
            SMAPI logs.
          </p>
        </div>
        <Button
          variant="secondary"
          size="sm"
          onClick={handleRefresh}
          disabled={refreshing || isLoading}
          className="flex items-center gap-1.5"
        >
          <RefreshCw
            className={`w-3.5 h-3.5 ${refreshing ? "animate-spin" : ""}`}
          />
          <span>Refresh</span>
        </Button>
      </div>

      {/* Health Findings Summary */}
      <Card className="space-y-4">
        <div className="flex items-center justify-between border-b border-[var(--border)] pb-3">
          <div className="flex items-center gap-2">
            <AlertTriangle className="w-4 h-4 text-amber-500" />
            <h3 className="font-bold text-sm">Active Findings & Health</h3>
          </div>
          <StatusBadge
            variant={
              counts.severities.error > 0
                ? "danger"
                : counts.severities.warning > 0
                  ? "warning"
                  : "success"
            }
          >
            {findings.length
              ? `${findings.length} Finding(s)`
              : "No known issues"}
          </StatusBadge>
        </div>

        {findings.length > 0 && (
          <fieldset className="flex flex-wrap items-center gap-2 text-xs">
            <legend className="sr-only">Filter findings</legend>
            {(["error", "warning", "info"] as SeverityKey[])
              .filter((key) => counts.severities[key] > 0)
              .map((key) => (
                <button
                  key={key}
                  type="button"
                  aria-pressed={severityFilter.has(key)}
                  onClick={() =>
                    setSeverityFilter(toggleIn(severityFilter, key))
                  }
                  className={`px-2 py-1 rounded-md border cursor-pointer ${
                    severityFilter.has(key)
                      ? "bg-[var(--accent-primary)] text-white border-transparent"
                      : "border-[var(--border)] text-[var(--fg-muted)]"
                  }`}
                >
                  {severityFilter.has(key) ? "\u2713 " : ""}
                  {key} ({counts.severities[key]})
                </button>
              ))}
            {[...counts.categories.entries()].map(([category, n]) => (
              <button
                key={category}
                type="button"
                aria-pressed={categoryFilter.has(category)}
                onClick={() =>
                  setCategoryFilter(toggleIn(categoryFilter, category))
                }
                className={`px-2 py-1 rounded-md border cursor-pointer ${
                  categoryFilter.has(category)
                    ? "bg-[var(--accent-primary)] text-white border-transparent"
                    : "border-[var(--border)] text-[var(--fg-muted)]"
                }`}
              >
                {categoryFilter.has(category) ? "\u2713 " : ""}
                {category} ({n})
              </button>
            ))}
            {(severityFilter.size > 0 || categoryFilter.size > 0) && (
              <button
                type="button"
                className="underline text-[var(--fg-muted)] cursor-pointer"
                onClick={() => {
                  setSeverityFilter(new Set());
                  setCategoryFilter(new Set());
                }}
              >
                Clear filters
              </button>
            )}
            <span className="text-[var(--fg-muted)]" aria-live="polite">
              Showing {visibleFindings.length} of {findings.length}
            </span>
          </fieldset>
        )}

        {findings.length > 0 ? (
          <div className="space-y-2">
            {visibleFindings.map((finding, idx) => (
              <div
                key={idx}
                className="p-3 rounded-lg border border-[var(--border)] bg-[var(--bg-elevated)]/20 flex items-start gap-3"
              >
                <AlertTriangle
                  className={`w-4 h-4 shrink-0 mt-0.5 ${
                    severityKey(finding.severity) === "error"
                      ? "text-[var(--danger)]"
                      : "text-amber-500"
                  }`}
                />
                <div className="space-y-1 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="font-mono font-bold text-xs">
                      {finding.code}
                    </span>
                    <span className="text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded bg-[var(--bg-elevated)] font-semibold">
                      {finding.severity}
                    </span>
                  </div>
                  <p className="text-xs font-semibold">{finding.title}</p>
                  <p className="text-xs text-[var(--fg-primary)] leading-relaxed">
                    {finding.summary}
                  </p>
                  {(finding.evidence.length > 0 ||
                    finding.affected_entities.length > 0) && (
                    <details className="text-xs text-[var(--fg-muted)]">
                      <summary className="cursor-pointer">
                        Evidence and affected items
                      </summary>
                      <p className="mt-1">
                        Category: {finding.category}. Observed{" "}
                        {finding.observed_at}.
                      </p>
                      {finding.evidence.length > 0 && (
                        <ul className="list-disc pl-4">
                          {finding.evidence.map((line) => (
                            <li key={line}>{redactText(line).text}</li>
                          ))}
                        </ul>
                      )}
                      {finding.affected_entities.length > 0 && (
                        <p className="font-mono break-all">
                          {finding.affected_entities.join(", ")}
                        </p>
                      )}
                    </details>
                  )}
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="flex items-center gap-2 text-xs text-emerald-600 dark:text-emerald-400 py-2">
            <CheckCircle2 className="w-4 h-4" />
            <span>
              No issues were found by the checks that ran. That is not proof the
              setup works, and some checks cannot run yet.
            </span>
          </div>
        )}
      </Card>

      {/* Platform report: the backend states what the binary actually is. */}
      <Card className="space-y-4">
        <div className="flex items-center justify-between border-b border-[var(--border)] pb-3">
          <div className="flex items-center gap-2">
            <MonitorCog className="w-4 h-4 text-[var(--accent-primary)]" />
            <h3 className="font-bold text-sm">Platform & Storage</h3>
          </div>
          {report?.host_operating_system && (
            <StatusBadge variant="info">
              {operatingSystemLabel(report.host_operating_system)}
            </StatusBadge>
          )}
        </div>

        <dl className="grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-3 text-xs">
          <div className="min-w-0">
            <dt className="text-[var(--fg-muted)] mb-0.5">Application data</dt>
            <dd className="font-mono break-all select-text">
              {report?.app_data_dir ?? "-"}
            </dd>
          </div>
          <div className="min-w-0">
            <dt className="text-[var(--fg-muted)] mb-0.5">Cache</dt>
            <dd className="font-mono break-all select-text">
              {report?.cache_dir ?? "-"}
            </dd>
          </div>
        </dl>

        {report?.steam_installations_checked &&
          report.steam_installations_checked.length > 0 && (
            <div className="space-y-1.5 pt-1 border-t border-[var(--border)]">
              <div className="flex items-center gap-2 pt-2">
                <HardDrive className="w-3.5 h-3.5 text-[var(--fg-muted)]" />
                <h4 className="text-xs font-semibold text-[var(--fg-muted)] uppercase tracking-wider">
                  Steam locations searched
                </h4>
              </div>
              <ul className="space-y-1">
                {report.steam_installations_checked.map((path) => (
                  <li
                    key={path}
                    className="font-mono text-xs break-all text-[var(--fg-muted)] select-text"
                  >
                    {path}
                  </li>
                ))}
              </ul>
            </div>
          )}

        {report?.smapi_log_locations &&
          report.smapi_log_locations.length > 0 && (
            <div className="space-y-1.5 pt-1 border-t border-[var(--border)]">
              <h4 className="text-xs font-semibold text-[var(--fg-muted)] uppercase tracking-wider pt-2">
                SMAPI log locations
              </h4>
              <ul className="space-y-1">
                {report.smapi_log_locations.map((location) => (
                  <li
                    key={location.context}
                    className="flex flex-wrap gap-x-2 text-xs select-text"
                  >
                    <span className="text-[var(--fg-muted)]">
                      {location.context}
                    </span>
                    <span className="font-mono break-all">{location.path}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
      </Card>

      <SupportExportCard
        report={report}
        overview={overview}
        mods={mods}
        managerVersion={bootstrap?.app_version}
      />

      {/* SMAPI Log Viewer */}
      <Card className="space-y-4">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 border-b border-[var(--border)] pb-3">
          <div className="flex items-center gap-2">
            <Terminal className="w-4 h-4 text-[var(--accent-primary)]" />
            <h3 className="font-bold text-sm">SMAPI Execution Log</h3>
          </div>

          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => handleCopyLog(true)}
              className="flex items-center gap-1.5"
            >
              <Copy className="w-3.5 h-3.5" />
              <span>
                {copied === "redacted"
                  ? "Copied (paths and secrets redacted)"
                  : copied === "failed"
                    ? "Clipboard unavailable"
                    : "Copy log (redacted)"}
              </span>
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => handleCopyLog(false)}
              title="Copies the log exactly as written, including personal paths"
            >
              {copied === "raw" ? "Copied (unredacted)" : "Copy original"}
            </Button>
          </div>
        </div>

        {report?.log_file_path && (
          <div className="text-xs text-[var(--fg-muted)] font-mono truncate select-text">
            Log path:{" "}
            <span className="text-[var(--fg-primary)]">
              {report.log_file_path}
            </span>
          </div>
        )}

        <div className="flex flex-wrap items-center gap-2 text-xs">
          <label className="flex items-center gap-2">
            <span className="text-[var(--fg-muted)]">Search log</span>
            <input
              type="search"
              value={logQuery}
              onChange={(event) => setLogQuery(event.target.value)}
              className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
            />
          </label>
          <span aria-live="polite" className="text-[var(--fg-muted)]">
            {matchingLines
              ? `${matchingLines.length} of ${logLines.length} line(s) match`
              : `${report?.raw_log ? logLines.length : 0} line(s)`}
          </span>
        </div>

        {matchingLines ? (
          <pre className="p-4 rounded-xl bg-[var(--bg-primary)] border border-[var(--border)] text-xs font-mono text-[var(--fg-muted)] overflow-x-auto max-h-96 select-text whitespace-pre-wrap leading-relaxed">
            {matchingLines.length > 0
              ? matchingLines
                  .map((line) => `${line.number}: ${line.text}`)
                  .join("\n")
              : "No lines match."}
          </pre>
        ) : (
          <pre className="p-4 rounded-xl bg-[var(--bg-primary)] border border-[var(--border)] text-xs font-mono text-[var(--fg-muted)] overflow-x-auto max-h-96 select-text whitespace-pre-wrap leading-relaxed">
            {report?.raw_log || "[SMAPI] No log output recorded yet."}
          </pre>
        )}
      </Card>
    </div>
  );
};
