import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useDiagnosticsReport,
  useActiveProfileOverview,
} from "@/shared/api/hooks";
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

  const [copied, setCopied] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  const handleCopyLog = async () => {
    if (!report?.raw_log) return;
    try {
      await navigator.clipboard.writeText(report.raw_log);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {}
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
              report?.findings.some((f) => f.severity === "Error")
                ? "danger"
                : report?.findings.some((f) => f.severity === "Warning")
                  ? "warning"
                  : "success"
            }
          >
            {report?.findings.length
              ? `${report.findings.length} Finding(s)`
              : "Healthy"}
          </StatusBadge>
        </div>

        {report?.findings && report.findings.length > 0 ? (
          <div className="space-y-2">
            {report.findings.map((finding, idx) => (
              <div
                key={idx}
                className="p-3 rounded-lg border border-[var(--border)] bg-[var(--bg-elevated)]/20 flex items-start gap-3"
              >
                <AlertTriangle
                  className={`w-4 h-4 shrink-0 mt-0.5 ${
                    finding.severity === "Error"
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
                  <p className="text-xs text-[var(--fg-primary)] leading-relaxed">
                    {finding.summary}
                  </p>
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="flex items-center gap-2 text-xs text-emerald-600 dark:text-emerald-400 py-2">
            <CheckCircle2 className="w-4 h-4" />
            <span>No health issues or compatibility errors detected.</span>
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
              onClick={handleCopyLog}
              className="flex items-center gap-1.5"
            >
              <Copy className="w-3.5 h-3.5" />
              <span>{copied ? "Copied!" : "Copy Log"}</span>
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

        <pre className="p-4 rounded-xl bg-[var(--bg-primary)] border border-[var(--border)] text-xs font-mono text-[var(--fg-muted)] overflow-x-auto max-h-96 select-text whitespace-pre-wrap leading-relaxed">
          {report?.raw_log || "[SMAPI] No log output recorded yet."}
        </pre>
      </Card>
    </div>
  );
};
