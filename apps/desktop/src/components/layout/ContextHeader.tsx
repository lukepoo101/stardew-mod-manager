import { smapiBadge } from "@/shared/smapi/status";
import React, { useState } from "react";
import { isRunningState } from "@/shared/launch/sessionResult";
import { useTheme } from "@/shared/theme/ThemeProvider";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { Button } from "@/components/ui/Button";
import { ProfileOverviewDto, LaunchSessionDto } from "@/shared/api/generated";
import {
  useActivateProfile,
  useDismissedFindings,
  useProfiles,
  useExperiments,
  useProfileFreeze,
} from "@/shared/api/hooks";
import { partitionFindings } from "@/shared/support/dismissals";
import { severityKey } from "@/shared/support/findings";
import { errorSummary } from "@/shared/api/errors";
import { Play, Square, AlertTriangle, Moon, Sun, Search } from "lucide-react";
import { Link } from "react-router-dom";

export const ContextHeader: React.FC<{
  overview?: ProfileOverviewDto;
  activeSession?: LaunchSessionDto | null;
  onLaunch?: (mode: "Modded" | "Vanilla") => void;
  onTerminate?: () => void;
  onSearch?: () => void;
  isLaunching?: boolean;
}> = ({
  overview,
  activeSession,
  onLaunch,
  onTerminate,
  onSearch,
  isLaunching,
}) => {
  const { resolvedTheme, toggleTheme } = useTheme();
  const { data: profiles } = useProfiles();
  const activate = useActivateProfile();
  const [switchError, setSwitchError] = useState<string | null>(null);
  const { data: freeze } = useProfileFreeze(overview?.profile.id);
  const { data: experiments } = useExperiments();
  const experiment = experiments?.find(
    (e) => e.profile_id === overview?.profile.id,
  );

  const isRunning = Boolean(
    activeSession && isRunningState(activeSession.state),
  );
  const isSmapiInstalled = Boolean(overview?.smapi_status.is_installed);
  const health = overview?.health_summary;
  const { data: dismissals } = useDismissedFindings();
  const visible = partitionFindings(
    health?.findings ?? [],
    dismissals ?? [],
  ).visible;
  const errorCount = visible.filter(
    (finding) => severityKey(finding.severity) === "error",
  ).length;
  const warningCount = visible.filter(
    (finding) => severityKey(finding.severity) === "warning",
  ).length;
  const totalFindings = warningCount + errorCount;
  const recoveryBlocked = Boolean(
    health?.findings.some((finding) => finding.code === "RECOVERY_REQUIRED"),
  );
  const modeLabel =
    activeSession?.launch_mode === "vanilla" ? "Vanilla" : "Modded";

  const switchProfile = async (profileId: string) => {
    if (!overview || profileId === overview.profile.id) return;
    setSwitchError(null);
    try {
      await activate.mutateAsync(profileId);
    } catch (error) {
      setSwitchError(
        `${errorSummary(error, "The profile was not switched")} "${overview.profile.name}" is still active.`,
      );
    }
  };
  const switchable = (profiles ?? []).filter(
    (profile) => profile.game_installation_id === overview?.game.id,
  );

  return (
    <>
      {/* Wraps onto a second line rather than overlapping when the window is
        narrow or the interface is scaled up. */}
      <header className="min-h-16 py-2 border-b border-[var(--border)] bg-[var(--bg-surface)] px-6 flex flex-wrap items-center justify-between gap-x-4 gap-y-2 sticky top-0 z-10 shadow-xs">
        {/* Left: Profile & Game context */}
        <div className="flex flex-[1_1_14rem] items-center gap-x-4 gap-y-1 min-w-0">
          {overview?.profile && (
            <div className="flex shrink-0 items-center gap-2">
              {/* The active profile, and switching it, in one control. */}
              {switchable.length > 1 ? (
                <select
                  aria-label="Active profile"
                  title="Switch the active profile"
                  value={overview.profile.id}
                  disabled={activate.isPending || isRunning}
                  onChange={(event) => void switchProfile(event.target.value)}
                  className="max-w-[14rem] truncate px-2.5 py-1 rounded-full bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] text-xs font-semibold border border-[var(--accent-primary)]/30 cursor-pointer disabled:cursor-not-allowed"
                >
                  {switchable.map((profile) => (
                    <option key={profile.id} value={profile.id}>
                      {profile.name}
                    </option>
                  ))}
                </select>
              ) : (
                <span className="px-2.5 py-1 rounded-full bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] text-xs font-semibold">
                  {overview.profile.name}
                </span>
              )}
              <span className="text-[11px] text-[var(--fg-muted)] font-mono">
                r{overview.profile.revision.toString()}
              </span>
              {experiment && (
                <span title={`Copy of ${experiment.source_name}`}>
                  <StatusBadge variant="warning">Experiment</StatusBadge>
                </span>
              )}
              {freeze && (
                <span title={freeze.reason || "Frozen"}>
                  <StatusBadge variant="info">Frozen</StatusBadge>
                </span>
              )}
            </div>
          )}

          {overview?.game?.canonical_root && (
            <div
              className="hidden @5xl:flex items-center gap-1.5 text-xs text-[var(--fg-muted)] font-mono min-w-0 max-w-xs @7xl:max-w-md"
              title={overview.game.canonical_root}
            >
              <span className="truncate">{overview.game.canonical_root}</span>
            </div>
          )}
        </div>

        {/* Right: Runtime status & Controls */}
        <div className="flex flex-wrap items-center gap-3">
          {onSearch && (
            <button
              type="button"
              onClick={onSearch}
              aria-label="Search"
              title="Search (Ctrl+K)"
              className="px-2.5 py-1.5 rounded-lg border border-[var(--border)] text-xs text-[var(--fg-muted)] hover:bg-[var(--bg-elevated)] cursor-pointer flex items-center gap-1.5 whitespace-nowrap"
            >
              <Search className="w-3.5 h-3.5" aria-hidden="true" />
              <span className="hidden @4xl:inline">Search</span>
              <kbd className="hidden @4xl:inline font-mono text-[10px]">
                Ctrl K
              </kbd>
            </button>
          )}

          {/* SMAPI Status */}
          <Link
            to="/app/overview#smapi"
            title="Manage SMAPI"
            aria-label={`${smapiBadge(overview?.smapi_status).label}. Manage SMAPI`}
          >
            <StatusBadge variant={smapiBadge(overview?.smapi_status).variant}>
              {smapiBadge(overview?.smapi_status).label}
            </StatusBadge>
          </Link>

          {/* Health Finding Pill */}
          {health && totalFindings > 0 && (
            <div
              className={`flex items-center gap-1.5 px-2 py-1 rounded-md text-xs font-medium whitespace-nowrap ${
                errorCount > 0
                  ? "bg-[var(--danger-surface)] text-[var(--danger)] border border-[var(--danger)]/30"
                  : "bg-amber-500/10 text-amber-500 border border-amber-500/30"
              }`}
            >
              <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
              <span>
                {errorCount} error(s), {warningCount} warning(s)
              </span>
            </div>
          )}

          {/* Quick Launch / Stop Game */}
          {isRunning ? (
            <Button
              variant="danger"
              size="sm"
              onClick={onTerminate}
              className="flex items-center gap-1.5"
            >
              <Square className="w-3.5 h-3.5 fill-current" />
              <span>Stop {modeLabel} game</span>
            </Button>
          ) : (
            <>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => onLaunch?.("Vanilla")}
                disabled={isLaunching || recoveryBlocked}
                title="Start the unmodified game. The active profile is not used."
              >
                <span>Vanilla</span>
              </Button>
              <Button
                variant="primary"
                size="sm"
                onClick={() => onLaunch?.("Modded")}
                disabled={!isSmapiInstalled || isLaunching || recoveryBlocked}
                isLoading={isLaunching}
                title={
                  recoveryBlocked
                    ? "Recovery is required before launching"
                    : `Launch with SMAPI using profile ${overview?.profile.name ?? ""}`
                }
                className="flex items-center gap-1.5 font-bold"
              >
                <Play className="w-3.5 h-3.5 fill-current" />
                <span>Launch modded</span>
              </Button>
            </>
          )}

          {/* Theme Toggle */}
          <button
            onClick={toggleTheme}
            className="p-2 rounded-lg hover:bg-[var(--bg-elevated)] border border-[var(--border)] text-sm cursor-pointer transition-colors"
            aria-label="Toggle theme"
            title={`Switch to ${resolvedTheme === "light" ? "dark" : "light"} mode`}
          >
            {resolvedTheme === "light" ? (
              <Moon className="w-4 h-4 text-[var(--fg-muted)]" />
            ) : (
              <Sun className="w-4 h-4 text-[var(--fg-muted)]" />
            )}
          </button>
        </div>
      </header>
      {switchError && (
        <p role="alert" className="px-6 pt-2 text-xs text-[var(--danger)]">
          {switchError}
        </p>
      )}
    </>
  );
};
