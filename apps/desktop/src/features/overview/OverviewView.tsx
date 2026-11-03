import {
  countsText,
  visibleHealthCounts,
} from "@/shared/health/profileFindings";
import React from "react";
import {
  describeRunningSession,
  isRunningState,
  type RunningResult,
} from "@/shared/launch/sessionResult";
import { useGuardedLaunch } from "@/shared/launch/useGuardedLaunch";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import {
  useActiveProfileOverview,
  useActiveLaunchSession,
  useLaunchPreflight,
  useTerminateSession,
  useProfileMods,
  useDismissedFindings,
  useQuickFileCheck,
} from "@/shared/api/hooks";
import { fileNotes } from "@/shared/launch/fileNotes";
import { isSteam, STEAM_NOTICE } from "@/shared/launch/steamNotice";
import { errorSummary } from "@/shared/api/errors";
import { operatingSystemLabel } from "@/shared/platform/labels";
import { ProfileModInstaller } from "@/features/mods/ProfileModInstaller";
import { LastSessionCard } from "./LastSessionCard";
import { FixPlanCard } from "./FixPlanCard";
import { SmapiPanel } from "@/features/smapi/SmapiPanel";
import { smapiBadge } from "@/shared/smapi/status";
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
  Info,
} from "lucide-react";

const RUNNING_TONE: Record<RunningResult["tone"], string> = {
  success:
    "bg-[var(--success-surface)] text-[var(--success)] border-[var(--success)]/20",
  warning:
    "bg-[var(--warning-surface)] text-[var(--warning)] border-[var(--warning)]/20",
  danger:
    "bg-[var(--danger-surface)] text-[var(--danger)] border-[var(--danger)]/20",
  info: "bg-[var(--bg-elevated)] text-[var(--fg-primary)] border-[var(--border)]",
  neutral:
    "bg-[var(--bg-elevated)] text-[var(--fg-muted)] border-[var(--border)]",
};

export const OverviewView: React.FC = () => {
  const {
    data: overview,
    error: overviewError,
    isFetching: overviewFetching,
    dataUpdatedAt: overviewCheckedAt,
  } = useActiveProfileOverview();
  const { data: activeSession } = useActiveLaunchSession();
  const { data: mods } = useProfileMods(overview?.profile.id);
  const { data: quickCheck } = useQuickFileCheck(overview?.profile.id);
  const changedFiles = fileNotes(quickCheck ?? []);

  const launchMutation = useGuardedLaunch();
  const terminateMutation = useTerminateSession();

  const isRunning = Boolean(
    activeSession && isRunningState(activeSession.state),
  );
  const running =
    isRunning && activeSession ? describeRunningSession(activeSession) : null;
  const isSmapiInstalled = Boolean(overview?.smapi_status.is_installed);
  const health = overview?.health_summary;
  // The same counts as the header and Diagnostics: dismissed findings are
  // left out and counted apart. Checks that could not be made are counted
  // apart from other information, so "unknown" is never read as "fine".
  const { data: dismissals } = useDismissedFindings();
  const counts = visibleHealthCounts(health?.findings ?? [], dismissals ?? []);

  const preflightEnabled = Boolean(overview) && !isRunning;
  const { data: moddedPreflight } = useLaunchPreflight(
    "Modded",
    preflightEnabled,
  );
  const { data: vanillaPreflight } = useLaunchPreflight(
    "Vanilla",
    preflightEnabled,
  );
  const moddedBlocked =
    !isSmapiInstalled || (moddedPreflight?.blockers.length ?? 0) > 0;
  const vanillaBlocked = (vanillaPreflight?.blockers.length ?? 0) > 0;
  // Reasons that stop only Play vanilla; shared ones are already listed.
  const vanillaOnlyBlockers = (vanillaPreflight?.blockers ?? []).filter(
    (blocker) => !moddedPreflight?.blockers.includes(blocker),
  );

  const handleLaunch = (mode: "Modded" | "Vanilla") => {
    void launchMutation.request(mode);
  };

  const handleTerminate = () => {
    terminateMutation.reset();
    terminateMutation.mutate(activeSession?.id);
  };

  return (
    <div className="space-y-6">
      {overviewError && (
        <p role="alert">
          {errorSummary(overviewError, "Unable to load the profile overview")}
        </p>
      )}
      {/* Top Banner / Hero Card */}
      <Card className="p-6 border-2 border-[var(--border)] bg-gradient-to-r from-[var(--bg-surface)] to-[var(--bg-elevated)]/30">
        <div className="flex flex-col @3xl:flex-row @3xl:items-center justify-between gap-6">
          <div className="space-y-2">
            <div className="flex flex-wrap items-center gap-2.5">
              <h2 className="text-2xl font-extrabold tracking-tight">
                {isRunning
                  ? "Game Running"
                  : moddedBlocked
                    ? "Not Ready for Modded Play"
                    : "Ready to Play"}
              </h2>
              <StatusBadge variant={smapiBadge(overview?.smapi_status).variant}>
                {smapiBadge(overview?.smapi_status).label}
              </StatusBadge>
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

          <div className="flex flex-wrap items-center gap-3">
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
              <>
                <Button
                  variant="secondary"
                  size="lg"
                  onClick={() => handleLaunch("Vanilla")}
                  disabled={vanillaBlocked || launchMutation.isPending}
                  title="Starts the game directly with no mods. This profile is not used."
                  className="flex items-center gap-2"
                >
                  <span>Play vanilla</span>
                </Button>
                <Button
                  variant="primary"
                  size="lg"
                  onClick={() => handleLaunch("Modded")}
                  disabled={moddedBlocked || launchMutation.isPending}
                  isLoading={launchMutation.isPending}
                  className="flex items-center gap-2 text-base px-8 font-bold"
                >
                  <Play className="w-5 h-5 fill-current" />
                  <span>Play modded</span>
                </Button>
              </>
            )}
          </div>
        </div>

        {launchMutation.dialog}
        {launchMutation.error && (
          <p role="alert" className="mt-3 text-xs text-[var(--danger)]">
            {launchMutation.error}
          </p>
        )}
        {isRunning && terminateMutation.error && (
          <p role="alert" className="mt-3 text-xs text-[var(--danger)]">
            {errorSummary(
              terminateMutation.error,
              "The game could not be stopped",
            )}
          </p>
        )}
        {!isRunning && (
          <div className="mt-4 space-y-2 text-xs">
            <p className="text-[var(--fg-muted)]">
              <strong>Play modded</strong> starts SMAPI with the mods in{" "}
              <span className="font-semibold">
                {overview?.profile.name ?? "this profile"}
              </span>
              . <strong>Play vanilla</strong> starts the unmodified game and
              ignores this profile.
            </p>
            {isSteam(overview?.game?.storefront) && (
              <p className="text-[var(--fg-muted)]">{STEAM_NOTICE}</p>
            )}
            {moddedPreflight && moddedPreflight.blockers.length > 0 && (
              <ul role="alert" className="text-[var(--danger)] space-y-0.5">
                {moddedPreflight.blockers.map((blocker) => (
                  <li key={blocker}>Blocked: {blocker}</li>
                ))}
              </ul>
            )}
            {vanillaOnlyBlockers.length > 0 && (
              <ul role="alert" className="text-[var(--danger)] space-y-0.5">
                {vanillaOnlyBlockers.map((blocker) => (
                  <li key={blocker}>Play vanilla blocked: {blocker}</li>
                ))}
              </ul>
            )}
            {moddedPreflight && moddedPreflight.warnings.length > 0 && (
              <ul className="text-[var(--warning)] space-y-0.5">
                {moddedPreflight.warnings.map((warning) => (
                  <li key={warning}>Note: {warning}</li>
                ))}
              </ul>
            )}
            {changedFiles.length > 0 && (
              <ul className="text-[var(--warning)] space-y-0.5">
                {changedFiles.map((note) => (
                  <li key={note}>Note: {note}</li>
                ))}
                <li>
                  <Link
                    to="/app/diagnostics"
                    className="text-[var(--accent-primary)] hover:underline"
                  >
                    Check mod files in Diagnostics →
                  </Link>
                </li>
              </ul>
            )}
          </div>
        )}

        {/* What SMAPI's log shows so far, coloured by the evidence */}
        {running && (
          <div
            role="status"
            className={`mt-4 p-3 rounded-lg border text-xs space-y-1 ${RUNNING_TONE[running.tone]}`}
          >
            <p className="flex items-center gap-2">
              {running.tone === "success" ? (
                <CheckCircle2 className="w-4 h-4 shrink-0" />
              ) : running.tone === "warning" || running.tone === "danger" ? (
                <AlertTriangle className="w-4 h-4 shrink-0" />
              ) : (
                <Info className="w-4 h-4 shrink-0" />
              )}
              <span>{running.text}</span>
            </p>
            {running.points.length > 0 && (
              <ul className="list-disc pl-10 space-y-0.5">
                {running.points.map((point) => (
                  <li key={point}>{point}</li>
                ))}
              </ul>
            )}
          </div>
        )}
      </Card>

      {!isRunning && <LastSessionCard />}
      {!isRunning && <FixPlanCard />}

      {/* Grid: Health / Profile Details / Quick Stats */}
      <div className="grid grid-cols-1 @4xl:grid-cols-3 gap-6">
        {/* Profile Card */}
        <Card className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)] pb-3">
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
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)] pb-3">
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
              <span>Platform:</span>
              <span className="font-medium text-[var(--fg-primary)]">
                {overview?.game?.operating_system
                  ? operatingSystemLabel(overview.game.operating_system)
                  : "Not registered"}
              </span>
            </div>
            {overview?.game?.management_mode === "external_unmanaged" && (
              <p role="note" className="text-[var(--warning)] pt-1">
                Not managed: this installation was added as it was. The manager
                does not install, change or remove SMAPI or files in its folder.
              </p>
            )}
          </div>
        </Card>

        {/* Health & Diagnostic Summary */}
        <Card className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--border)] pb-3">
            <div className="flex items-center gap-2">
              <AlertTriangle className="w-4 h-4 text-emerald-500" />
              <h3 className="font-bold text-sm">Compatibility & Health</h3>
            </div>
            <StatusBadge
              variant={
                !health
                  ? "neutral"
                  : counts.errors > 0
                    ? "danger"
                    : counts.warnings > 0
                      ? "warning"
                      : counts.visible.length > 0
                        ? "info"
                        : "success"
              }
            >
              {!health
                ? "Not checked yet"
                : health.findings.length > 0
                  ? countsText(counts, true)
                  : "No known issues"}
            </StatusBadge>
          </div>
          <div className="space-y-2 text-xs text-[var(--fg-muted)]">
            {health && overviewError && (
              <p role="status" className="text-[var(--warning)]">
                Could not refresh the checks. These results are from{" "}
                {new Date(overviewCheckedAt).toLocaleString()} and may be out of
                date.
              </p>
            )}
            {health && !overviewError && overviewFetching && (
              <p role="status">Rechecking...</p>
            )}
            {!health ? (
              <p role="status">
                {overviewError
                  ? "The health summary could not be loaded."
                  : "Checking..."}
              </p>
            ) : counts.visible.length > 0 ? (
              <ul className="space-y-1">
                {counts.visible.slice(0, 2).map((f, i) => (
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
                <span>
                  No issues found by the checks that ran. This is not proof of
                  compatibility.
                </span>
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

      <SmapiPanel />

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
