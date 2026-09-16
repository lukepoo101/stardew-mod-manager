import React from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useActiveProfileOverview,
  useActiveLaunchSession,
  useLaunchGame,
  useTerminateSession,
  useProfileMods,
} from "@/shared/api/hooks";
import { ProfileModInstaller } from "@/features/mods/ProfileModInstaller";
import { Link } from "react-router-dom";
import {
  Play,
  Square,
  Package,
  CheckCircle2,
  AlertTriangle,
  Folder,
  Layers,
  ArrowRight,
} from "lucide-react";

export const OverviewView: React.FC = () => {
  const { data: overview, error: overviewError } = useActiveProfileOverview();
  const { data: activeSession } = useActiveLaunchSession();
  const { data: mods } = useProfileMods(overview?.profile.id);

  const launchMutation = useLaunchGame();
  const terminateMutation = useTerminateSession();

  const isRunning = Boolean(
    activeSession &&
      activeSession.state !== "failed" &&
      activeSession.state !== "exited",
  );
  const isSmapiInstalled = Boolean(overview?.smapi_status.is_installed);
  const health = overview?.health_summary;

  const handleLaunch = () => {
    launchMutation.mutate("Modded");
  };

  const handleTerminate = () => {
    terminateMutation.mutate(activeSession?.id);
  };

  return (
    <div className="space-y-6">
      {overviewError && <p role="alert">{overviewError.message}</p>}
      {/* Top Banner / Hero Card */}
      <Card className="p-6 border-2 border-[var(--border)] bg-gradient-to-r from-[var(--bg-surface)] to-[var(--bg-elevated)]/30">
        <div className="flex flex-col md:flex-row md:items-center justify-between gap-6">
          <div className="space-y-2">
            <div className="flex items-center gap-2.5">
              <h2 className="text-2xl font-extrabold tracking-tight">
                {isRunning ? "Game Running" : "Ready to Play"}
              </h2>
              {isSmapiInstalled ? (
                <StatusBadge variant="success">
                  SMAPI {overview?.smapi_status.observed_version || "4.1.10"}
                </StatusBadge>
              ) : (
                <StatusBadge variant="warning">SMAPI Missing</StatusBadge>
              )}
            </div>
            <p className="text-sm text-[var(--fg-muted)]">
              {mods?.length ?? overview?.mod_count ?? 0} mod(s) installed in
              profile{" "}
              <span className="font-semibold text-[var(--fg-primary)]">
                {overview?.profile.name || "Default Profile"}
              </span>{" "}
              (revision {overview?.profile.revision.toString() || "1"})
            </p>
          </div>

          <div className="flex items-center gap-3">
            {isRunning ? (
              <Button
                variant="danger"
                size="lg"
                onClick={handleTerminate}
                isLoading={terminateMutation.isPending}
                className="flex items-center gap-2"
              >
                <Square className="w-4 h-4 fill-current" />
                <span>Stop Game</span>
              </Button>
            ) : (
              <Button
                variant="primary"
                size="lg"
                onClick={handleLaunch}
                disabled={!isSmapiInstalled || launchMutation.isPending}
                isLoading={launchMutation.isPending}
                className="flex items-center gap-2 text-base px-8 font-bold"
              >
                <Play className="w-5 h-5 fill-current" />
                <span>Play</span>
              </Button>
            )}
          </div>
        </div>

        {/* Active Session verification pill */}
        {activeSession && activeSession.verification_details && (
          <div className="mt-4 p-3 rounded-lg bg-emerald-500/10 border border-emerald-500/20 flex items-center gap-2 text-xs text-emerald-600 dark:text-emerald-400">
            <CheckCircle2 className="w-4 h-4 shrink-0" />
            <span>{activeSession.verification_details}</span>
          </div>
        )}
      </Card>

      {/* Grid: Health / Profile Details / Quick Stats */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
        {/* Profile Card */}
        <Card className="space-y-4">
          <div className="flex items-center justify-between border-b border-[var(--border)] pb-3">
            <div className="flex items-center gap-2">
              <Layers className="w-4 h-4 text-[var(--accent-primary)]" />
              <h3 className="font-bold text-sm">Active Profile</h3>
            </div>
            <Link
              to="/app/profiles"
              className="text-xs text-[var(--accent-primary)] hover:underline font-medium flex items-center gap-1"
            >
              Switch <ArrowRight className="w-3 h-3" />
            </Link>
          </div>
          <div className="space-y-2 text-xs text-[var(--fg-muted)]">
            <div className="flex justify-between">
              <span>Name:</span>
              <span className="font-semibold text-[var(--fg-primary)]">
                {overview?.profile.name}
              </span>
            </div>
            <div className="flex justify-between">
              <span>Revision:</span>
              <span className="font-mono text-[var(--fg-primary)]">
                r{overview?.profile.revision.toString()}
              </span>
            </div>
            <div className="flex justify-between">
              <span>Installed Mods:</span>
              <span className="font-semibold text-[var(--fg-primary)]">
                {overview?.mod_count ?? 0}
              </span>
            </div>
          </div>
        </Card>

        {/* Game Installation Card */}
        <Card className="space-y-4">
          <div className="flex items-center justify-between border-b border-[var(--border)] pb-3">
            <div className="flex items-center gap-2">
              <Folder className="w-4 h-4 text-amber-500" />
              <h3 className="font-bold text-sm">Game Directory</h3>
            </div>
            <StatusBadge variant="info">
              {overview?.game?.storefront || "Steam"}
            </StatusBadge>
          </div>
          <div className="space-y-2 text-xs text-[var(--fg-muted)]">
            <p
              className="font-mono truncate select-text"
              title={overview?.game?.canonical_root}
            >
              {overview?.game?.canonical_root || "No game selected"}
            </p>
            <div className="flex justify-between pt-1">
              <span>OS / Platform:</span>
              <span className="font-medium text-[var(--fg-primary)]">
                {overview?.game?.operating_system || "Linux"}
              </span>
            </div>
          </div>
        </Card>

        {/* Health & Diagnostic Summary */}
        <Card className="space-y-4">
          <div className="flex items-center justify-between border-b border-[var(--border)] pb-3">
            <div className="flex items-center gap-2">
              <AlertTriangle className="w-4 h-4 text-emerald-500" />
              <h3 className="font-bold text-sm">Compatibility & Health</h3>
            </div>
            <StatusBadge
              variant={
                health && health.error_count > 0
                  ? "danger"
                  : health && health.warning_count > 0
                    ? "warning"
                    : "success"
              }
            >
              {health?.status || "Healthy"}
            </StatusBadge>
          </div>
          <div className="space-y-2 text-xs text-[var(--fg-muted)]">
            {health && health.findings.length > 0 ? (
              <ul className="space-y-1">
                {health.findings.slice(0, 2).map((f, i) => (
                  <li
                    key={i}
                    className="flex items-start gap-1.5 text-[var(--fg-primary)]"
                  >
                    <span className="text-amber-500">•</span>
                    <span className="truncate">{f.summary}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-emerald-600 dark:text-emerald-400 flex items-center gap-1.5">
                <CheckCircle2 className="w-3.5 h-3.5" />
                <span>No blocking conflicts or issues detected.</span>
              </p>
            )}
            <Link
              to="/app/diagnostics"
              className="text-xs text-[var(--accent-primary)] hover:underline font-medium block pt-1"
            >
              View diagnostics report →
            </Link>
          </div>
        </Card>
      </div>

      {/* Mod Quick Install / Drop Zone */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-base font-bold tracking-tight flex items-center gap-2">
            <Package className="w-4 h-4 text-[var(--accent-primary)]" />
            <span>Install Mods</span>
          </h3>
          <Link
            to="/app/mods"
            className="text-xs text-[var(--accent-primary)] hover:underline font-medium"
          >
            Manage all mods ({mods?.length ?? 0}) →
          </Link>
        </div>

        {overview && (
          <ProfileModInstaller
            key={overview.profile.id}
            profileId={overview.profile.id}
          />
        )}
      </div>
    </div>
  );
};
