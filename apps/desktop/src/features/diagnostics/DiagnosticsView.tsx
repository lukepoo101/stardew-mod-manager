import { matchSkipped } from "@/shared/diagnostics/skipped";
import { usePreferences } from "@/shared/preferences";
import React, { useMemo, useState } from "react";
import { Card } from "@/components/ui/Card";
import { LoadFailed } from "@/components/ui/EmptyState";
import { errorSummary } from "@/shared/api/errors";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useBootstrap,
  useDiagnosticsReport,
  useActiveProfileOverview,
  useProfileMods,
  useRecentOperations,
  useDismissedFindings,
} from "@/shared/api/hooks";
import { api } from "@/shared/api/client";
import {
  canDismiss,
  findingSignature,
  partitionFindings,
} from "@/shared/support/dismissals";
import type { FindingDto } from "@/shared/api/generated";
import { redactText } from "@/shared/support/redact";
import { guidanceFor } from "@/shared/health/guidance";
import { Link } from "react-router-dom";
import { copyText } from "@/shared/support/actions";
import {
  EMPTY_FILTER,
  filterFindings,
  findingCounts,
  severityKey,
  type SeverityKey,
} from "@/shared/support/findings";
import { CopyButton } from "@/components/ui/CopyButton";
import { LogParserCard } from "./LogParserCard";
import { ModFilesCard } from "./ModFilesCard";
import { TroubleshootCard } from "./TroubleshootCard";
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
  const {
    data: report,
    isLoading,
    error: reportError,
    refetch,
  } = useDiagnosticsReport(gameId);
  const { data: bootstrap } = useBootstrap();
  const { data: mods } = useProfileMods(overview?.profile.id);
  const { data: operations } = useRecentOperations(100);

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
  const installedIds = useMemo(
    () => new Set((mods ?? []).map((mod) => mod.unique_id.toLowerCase())),
    [mods],
  );
  const { data: dismissals } = useDismissedFindings();
  const [showDismissed, setShowDismissed] = useState(false);
  const [preferences] = usePreferences();
  const [showQuiet, setShowQuiet] = useState(false);
  const allFindings = report?.findings ?? [];
  const partition = partitionFindings(allFindings, dismissals ?? []);
  const findings = partition.visible;
  const setDismissed = async (finding: FindingDto, dismiss: boolean) => {
    try {
      if (dismiss) {
        await api.dismissFinding(
          finding.fingerprint,
          findingSignature(finding),
          finding.severity,
        );
      } else {
        await api.restoreFinding(finding.fingerprint);
      }
    } catch {
      // The finding simply stays as it was; nothing is hidden on failure.
    }
  };
  const counts = findingCounts(findings);
  const filtered = filterFindings(findings, {
    ...EMPTY_FILTER,
    severities: severityFilter,
    categories: categoryFilter,
  });
  // Quiet mode hides only informational findings, and only when the user has
  // not asked for a severity explicitly. Nothing is reclassified.
  const quiet =
    preferences.quietInfo && !showQuiet && severityFilter.size === 0;
  const quietHidden = quiet
    ? filtered.filter((f) => severityKey(f.severity) === "info").length
    : 0;
  const visibleFindings = quiet
    ? filtered.filter((f) => severityKey(f.severity) !== "info")
    : filtered;
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
              !report
                ? "neutral"
                : counts.severities.error > 0
                  ? "danger"
                  : counts.severities.warning > 0
                    ? "warning"
                    : "success"
            }
          >
            {!report
              ? reportError
                ? "Not checked"
                : "Checking..."
              : findings.length
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

        {quietHidden > 0 && (
          <p className="text-xs text-[var(--fg-muted)]">
            {quietHidden} informational finding(s) hidden by your quieter health
            setting.{" "}
            <button
              type="button"
              className="underline cursor-pointer"
              onClick={() => setShowQuiet(true)}
            >
              Show them
            </button>
          </p>
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
                  <div className="flex items-start justify-between gap-2">
                    <p className="text-xs font-semibold">{finding.title}</p>
                    {canDismiss(finding) && (
                      <button
                        type="button"
                        className="text-[11px] underline text-[var(--fg-muted)] cursor-pointer shrink-0"
                        onClick={() => setDismissed(finding, true)}
                        title="Hide this until it changes. The problem itself is not fixed."
                      >
                        Dismiss
                      </button>
                    )}
                  </div>
                  <p className="text-xs text-[var(--fg-primary)] leading-relaxed">
                    {finding.summary}
                  </p>
                  {(() => {
                    const guide = guidanceFor(finding);
                    return (
                      <div className="text-xs space-y-1">
                        {guide.impact && (
                          <p>
                            <span className="font-semibold">
                              Why it matters:{" "}
                            </span>
                            {guide.impact}
                          </p>
                        )}
                        <p>
                          <span className="font-semibold">What to do: </span>
                          {guide.action ? (
                            <Link
                              to={guide.action.to}
                              className="text-[var(--accent-primary)] hover:underline"
                            >
                              {guide.action.label}
                            </Link>
                          ) : (
                            "Manual investigation required. The evidence below is the starting point."
                          )}
                        </p>
                        <p className="text-[var(--fg-muted)]">
                          Source: {guide.source}.{" "}
                          {guide.certainty === "inferred"
                            ? "This is an inference from what was observed, not a detected failure."
                            : "Observed directly."}
                        </p>
                      </div>
                    );
                  })()}
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
        ) : !report ? (
          reportError ? (
            <LoadFailed
              what="the health report"
              message={errorSummary(reportError)}
              onRetry={() => void refetch()}
            />
          ) : (
            <p role="status" className="text-xs text-[var(--fg-muted)] py-2">
              Running checks...
            </p>
          )
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

      {partition.dismissed.length > 0 && (
        <Card className="space-y-2">
          <button
            type="button"
            className="text-xs font-semibold cursor-pointer"
            aria-expanded={showDismissed}
            onClick={() => setShowDismissed(!showDismissed)}
          >
            Dismissed findings ({partition.dismissed.length})
          </button>
          {showDismissed && (
            <ul className="space-y-1.5">
              {partition.dismissed.map((finding) => (
                <li
                  key={finding.fingerprint}
                  className="text-xs flex items-start justify-between gap-2"
                >
                  <span>
                    <span className="font-semibold">{finding.title}</span>{" "}
                    <span className="text-[var(--fg-muted)]">
                      {finding.summary}
                    </span>
                  </span>
                  <button
                    type="button"
                    className="underline cursor-pointer shrink-0"
                    onClick={() => setDismissed(finding, false)}
                  >
                    Show again
                  </button>
                </li>
              ))}
            </ul>
          )}
          <p className="text-[11px] text-[var(--fg-muted)]">
            A dismissed finding returns by itself if what it reports changes.
          </p>
        </Card>
      )}

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
              {report?.app_data_dir && (
                <CopyButton
                  value={report.app_data_dir}
                  label="application data path"
                />
              )}
            </dd>
          </div>
          <div className="min-w-0">
            <dt className="text-[var(--fg-muted)] mb-0.5">Cache</dt>
            <dd className="font-mono break-all select-text">
              {report?.cache_dir ?? "-"}
              {report?.cache_dir && (
                <CopyButton value={report.cache_dir} label="cache path" />
              )}
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

      {/* What the SMAPI log says about the latest session */}
      {report && report.log_summary.total_lines > 0 && (
        <Card className="space-y-3">
          <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
            <Terminal className="w-4 h-4 text-[var(--accent-primary)]" />
            <h3 className="font-bold text-sm">Latest session summary</h3>
          </div>
          <p className="text-xs text-[var(--fg-muted)]">
            {report.log_summary.smapi_version
              ? `SMAPI ${report.log_summary.smapi_version}`
              : "SMAPI version not found in the log"}
            {report.log_summary.game_version
              ? ` with Stardew Valley ${report.log_summary.game_version}`
              : ""}
            {report.log_summary.loaded_mod_count !== null
              ? `. ${report.log_summary.loaded_mod_count} mods loaded.`
              : ". Loaded mod count not found."}
          </p>

          {report.log_summary.skipped_mods.length > 0 && (
            <div className="space-y-1">
              <h4 className="text-xs font-semibold">
                Mods that did not load ({report.log_summary.skipped_mods.length}
                )
              </h4>
              <ul className="space-y-1.5">
                {matchSkipped(report.log_summary.skipped_mods, mods ?? []).map(
                  (match) => {
                    const skipped = match.skipped;
                    return (
                      <li
                        key={`${skipped.name}-${skipped.line}`}
                        className="text-xs p-2 rounded border border-[var(--border)]"
                      >
                        <span className="font-semibold">{skipped.name}</span>
                        <span className="text-[var(--fg-muted)]">
                          {" "}
                          (
                          {match.kind === "exact"
                            ? "matches an installed mod"
                            : match.kind === "likely"
                              ? `probably ${match.mod.name} ${match.mod.version}; the version differs or was not logged`
                              : "not matched to an installed mod"}
                          )
                        </span>
                        {skipped.version ? ` ${skipped.version}` : ""}
                        {skipped.reason ? `: ${skipped.reason}` : ""}
                        {skipped.missing_dependencies.map((id) => {
                          const installed = installedIds.has(id.toLowerCase());
                          return (
                            <span
                              key={id}
                              className="block text-[var(--fg-muted)]"
                            >
                              Needs <span className="font-mono">{id}</span>
                              {installed
                                ? " (installed in this profile, so check its version or whether it failed to load)"
                                : " (not installed in this profile)"}
                            </span>
                          );
                        })}
                        <button
                          type="button"
                          className="underline text-[var(--fg-muted)] cursor-pointer mt-1"
                          onClick={() =>
                            setLogQuery(
                              logLines[skipped.line - 1]?.trim() ?? "",
                            )
                          }
                        >
                          Show log line {skipped.line}
                        </button>
                      </li>
                    );
                  },
                )}
              </ul>
            </div>
          )}

          {report.log_summary.sources.length > 0 && (
            <div className="space-y-1">
              <h4 className="text-xs font-semibold">
                Errors and warnings by source
              </h4>
              <ul className="text-xs space-y-0.5">
                {report.log_summary.sources.map((source) => (
                  <li key={source.source}>
                    <span className="font-semibold">{source.source}</span>:{" "}
                    {source.errors} error(s), {source.warnings} warning(s)
                    {source.first_error_line !== null && (
                      <button
                        type="button"
                        className="underline ml-2 text-[var(--fg-muted)] cursor-pointer"
                        onClick={() =>
                          setLogQuery(
                            logLines[
                              (source.first_error_line ?? 1) - 1
                            ]?.trim() ?? "",
                          )
                        }
                      >
                        First error (line {source.first_error_line})
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {report.log_summary.update_notices.length > 0 && (
            <div className="space-y-1">
              <h4 className="text-xs font-semibold">
                Newer versions reported by SMAPI
              </h4>
              <ul className="text-xs space-y-0.5">
                {report.log_summary.update_notices.map((notice) => (
                  <li key={`${notice.name}-${notice.line}`}>
                    {notice.name}: {notice.current_version} to{" "}
                    {notice.available_version}
                  </li>
                ))}
              </ul>
              <p className="text-[11px] text-[var(--fg-muted)]">
                Reported by SMAPI, not checked by this manager.
              </p>
            </div>
          )}

          {report.log_summary.skipped_mods.length === 0 &&
            report.log_summary.sources.length === 0 && (
              <p className="text-xs text-[var(--fg-muted)]">
                No skipped mods or errors were recognised in this log. Anything
                the summary does not recognise is still in the full log below.
              </p>
            )}
        </Card>
      )}

      <TroubleshootCard />
      {report && <LogParserCard rawLog={report.raw_log} />}
      <ModFilesCard />

      <SupportExportCard
        report={report}
        overview={overview}
        mods={mods}
        operations={operations}
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
