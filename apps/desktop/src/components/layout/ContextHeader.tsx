import React from "react";
import { useTheme } from "@/shared/theme/ThemeProvider";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { Button } from "@/components/ui/Button";
import {
  ProfileOverviewDto,
  LaunchSessionSummaryDto,
} from "@/shared/api/generated";
import { Play, Square, AlertTriangle, Moon, Sun } from "lucide-react";

export const ContextHeader: React.FC<{
  overview?: ProfileOverviewDto;
  activeSession?: LaunchSessionSummaryDto | null;
  onLaunch?: () => void;
  onTerminate?: () => void;
  isLaunching?: boolean;
}> = ({ overview, activeSession, onLaunch, onTerminate, isLaunching }) => {
  const { resolvedTheme, toggleTheme } = useTheme();

  const isRunning = Boolean(
    activeSession &&
      activeSession.state !== "Terminated" &&
      activeSession.state !== "Exited"
  );
  const isSmapiInstalled = Boolean(overview?.smapi_status.is_installed);
  const health = overview?.health_summary;
  const totalFindings = (health?.warning_count ?? 0) + (health?.error_count ?? 0);

  return (
    <header className="h-16 border-b border-[var(--border)] bg-[var(--bg-surface)] px-6 flex items-center justify-between sticky top-0 z-10 shadow-xs">
      {/* Left: Profile & Game context */}
      <div className="flex items-center gap-4 min-w-0">
        {overview?.profile && (
          <div className="flex items-center gap-2">
            <span className="px-2.5 py-1 rounded-full bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] text-xs font-semibold">
              {overview.profile.name}
            </span>
            <span className="text-[11px] text-[var(--fg-muted)] font-mono">
              r{overview.profile.revision.toString()}
            </span>
          </div>
        )}

        {overview?.game?.canonical_root && (
          <div className="hidden md:flex items-center gap-1.5 text-xs text-[var(--fg-muted)] font-mono truncate max-w-xs lg:max-w-md">
            <span className="truncate">{overview.game.canonical_root}</span>
          </div>
        )}
      </div>

      {/* Right: Runtime status & Controls */}
      <div className="flex items-center gap-3">
        {/* SMAPI Status */}
        {isSmapiInstalled ? (
          <StatusBadge variant="success">
            SMAPI {overview?.smapi_status.observed_version || overview?.smapi_status.tested_version}
          </StatusBadge>
        ) : (
          <StatusBadge variant="warning">No SMAPI</StatusBadge>
        )}

        {/* Health Finding Pill */}
        {health && totalFindings > 0 && (
          <div
            className={`flex items-center gap-1.5 px-2 py-1 rounded-md text-xs font-medium ${
              health.error_count > 0
                ? "bg-[var(--danger-surface)] text-[var(--danger)] border border-[var(--danger)]/30"
                : "bg-amber-500/10 text-amber-500 border border-amber-500/30"
            }`}
          >
            <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
            <span>
              {totalFindings} {totalFindings === 1 ? "issue" : "issues"}
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
            <span>Stop Game</span>
          </Button>
        ) : (
          <Button
            variant="primary"
            size="sm"
            onClick={onLaunch}
            disabled={!isSmapiInstalled || isLaunching}
            isLoading={isLaunching}
            className="flex items-center gap-1.5 font-bold"
          >
            <Play className="w-3.5 h-3.5 fill-current" />
            <span>Play</span>
          </Button>
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
  );
};
