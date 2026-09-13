import React, { useState, useEffect, useCallback, useRef } from "react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { GameInstallation, InstalledMod, LaunchSession, Setup } from "@/lib/backend/types";
import { backend } from "@/lib/backend/client";

export interface LaunchPanelProps {
  game: GameInstallation;
  setup: Setup;
  installedMods: InstalledMod[];
  initialSession?: LaunchSession;
}

export const LaunchPanel: React.FC<LaunchPanelProps> = ({
  game,
  setup,
  installedMods,
  initialSession,
}) => {
  const [session, setSession] = useState<LaunchSession | null>(initialSession || null);
  const [isLaunching, setIsLaunching] = useState(false);
  const [isTerminating, setIsTerminating] = useState(false);
  const [showLogs, setShowLogs] = useState(false);
  const [logContent, setLogContent] = useState<string>("");
  const [logPath, setLogPath] = useState<string>("");
  const [isLoadingLogs, setIsLoadingLogs] = useState(false);
  const [isCopied, setIsCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const logContainerRef = useRef<HTMLDivElement>(null);

  // Sync initialSession if provided
  useEffect(() => {
    if (initialSession && !session) {
      setSession(initialSession);
    }
  }, [initialSession]);

  const isRunning =
    session != null &&
    (session.state === "starting" ||
      session.state === "running_unverified" ||
      session.state === "mod_load_confirmed" ||
      session.state === "verification_unavailable");

  // Fetch SMAPI logs
  const fetchLogs = useCallback(async () => {
    setIsLoadingLogs(true);
    try {
      const [content, path] = await Promise.all([
        backend.getSmapiLog(),
        backend.getSmapiLogPath(),
      ]);
      setLogContent(content);
      setLogPath(path);
    } catch (e: any) {
      console.error("Failed to load SMAPI logs:", e);
    } finally {
      setIsLoadingLogs(false);
    }
  }, []);

  // When log section is opened, fetch logs immediately
  useEffect(() => {
    if (showLogs) {
      fetchLogs();
    }
  }, [showLogs, fetchLogs]);

  // Session Polling Effect
  useEffect(() => {
    if (!session || session.state === "exited" || session.state === "failed") {
      return;
    }

    let isMounted = true;
    const poll = async () => {
      try {
        const updated = await backend.pollSession(session.id);
        if (isMounted && updated) {
          setSession(updated);
          // If logs are shown and game is running, keep logs refreshed
          if (showLogs) {
            backend.getSmapiLog().then((content) => {
              if (isMounted && content) {
                setLogContent(content);
              }
            });
          }
        }
      } catch (err) {
        console.error("Error polling session:", err);
      }
    };

    poll();
    const interval = setInterval(poll, 1000);

    return () => {
      isMounted = false;
      clearInterval(interval);
    };
  }, [session?.id, session?.state, showLogs]);

  const handlePlay = async () => {
    setIsLaunching(true);
    setError(null);
    try {
      const activeSession = await backend.launchGame(game.id, setup.id);
      setSession(activeSession);
      if (showLogs) {
        setTimeout(fetchLogs, 1500);
      }
    } catch (e: any) {
      setError(e?.toString() || "Failed to launch game");
    } finally {
      setIsLaunching(false);
    }
  };

  const handleForceClose = async () => {
    setIsTerminating(true);
    setError(null);
    try {
      await backend.terminateGame(session?.id);
      if (session) {
        setSession({ ...session, state: "exited" });
      }
      if (showLogs) {
        setTimeout(fetchLogs, 500);
      }
    } catch (e: any) {
      setError(e?.toString() || "Failed to stop game");
    } finally {
      setIsTerminating(false);
    }
  };

  const handleCopyLogs = async () => {
    if (!logContent) return;
    try {
      await navigator.clipboard.writeText(logContent);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);
    } catch (err) {
      console.error("Clipboard copy failed:", err);
    }
  };

  return (
    <Card className="space-y-4 bg-gradient-to-br from-[var(--bg-surface)] to-[var(--bg-elevated)] border-2">
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h3 className="text-lg font-bold text-[var(--fg-primary)] flex items-center gap-2">
            Ready to Play
            {session ? (
              session.state === "mod_load_confirmed" ? (
                <StatusBadge variant="success">Mod Loaded &amp; Verified</StatusBadge>
              ) : session.state === "running_unverified" ? (
                <StatusBadge variant="warning">Verifying Mod Load...</StatusBadge>
              ) : session.state === "verification_unavailable" ? (
                <StatusBadge variant="neutral">Verification Unavailable</StatusBadge>
              ) : session.state === "exited" ? (
                <StatusBadge variant="neutral">Game Closed</StatusBadge>
              ) : (
                <StatusBadge variant="info">{session.state}</StatusBadge>
              )
            ) : (
              <StatusBadge variant="neutral">Game Closed</StatusBadge>
            )}
          </h3>
          <p className="text-xs text-[var(--fg-muted)] mt-0.5">
            {installedMods.length} mod{installedMods.length === 1 ? "" : "s"} enabled in managed profile
          </p>
        </div>

        <div className="flex items-center gap-2">
          {isRunning && (
            <Button
              variant="danger"
              size="lg"
              isLoading={isTerminating}
              disabled={isTerminating}
              onClick={handleForceClose}
              className="font-bold shadow-md hover:scale-[1.02] active:scale-[0.98] transition-transform flex items-center gap-1.5"
              title="Force terminate the running Stardew Valley process"
            >
              <span>⏹</span> Stop Game
            </Button>
          )}

          <Button
            variant="primary"
            size="lg"
            isLoading={isLaunching}
            disabled={isRunning || isLaunching}
            onClick={handlePlay}
            className="min-w-[160px] text-lg font-bold shadow-md hover:scale-[1.02] active:scale-[0.98] transition-transform"
          >
            {isRunning ? "Game Running" : "▶ Play"}
          </Button>
        </div>
      </div>

      {session?.verification_result && (
        <div className="p-3 bg-[var(--success-surface)] border border-[var(--success)]/20 rounded-lg text-xs text-[var(--success)]">
          <div className="font-bold flex items-center gap-1.5">
            <span>✓</span> Mod load confirmed:
          </div>
          <div>{session.verification_result.details}</div>
          {session.verification_result.confirmed_mods.length > 0 && (
            <div className="mt-1 font-mono text-[11px] text-[var(--fg-primary)] opacity-90">
              Confirmed loaded mods: {session.verification_result.confirmed_mods.join(", ")}
            </div>
          )}
        </div>
      )}

      {session?.state === "verification_unavailable" && (
        <div className="p-3 bg-[var(--warning-surface)] border border-[var(--warning)]/20 rounded-lg text-xs text-[var(--warning)]">
          <strong>Notice:</strong> Game launched, but SMAPI log verification timed out after 60 seconds. The game remains running normally.
        </div>
      )}

      {error && (
        <div className="p-3 bg-[var(--danger-surface)] border border-[var(--danger)]/30 rounded-lg text-xs text-[var(--danger)]">
          {error}
        </div>
      )}

      <div className="pt-2 border-t border-[var(--border)] flex justify-between items-center text-xs">
        <button
          onClick={() => setShowLogs(!showLogs)}
          className="text-[var(--fg-muted)] hover:text-[var(--fg-primary)] underline cursor-pointer font-medium"
        >
          {showLogs ? "Hide Diagnostics & SMAPI Logs" : "Troubleshooting & SMAPI Logs"}
        </button>
        <span className="text-[var(--fg-muted)]">Launch spec: direct SMAPI --mods-path</span>
      </div>

      {showLogs && (
        <div className="space-y-3 pt-1">
          {/* Metadata Bar */}
          <div className="p-3 bg-[var(--bg-elevated)] border border-[var(--border)] rounded-lg text-xs space-y-1 font-mono text-[var(--fg-primary)]">
            <div className="flex justify-between items-center">
              <span className="text-[var(--fg-muted)]">Target:</span>
              <span className="truncate max-w-md">{game.canonical_root}/StardewModdingAPI</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[var(--fg-muted)]">Mods Dir:</span>
              <span className="truncate max-w-md">{setup.relative_mods_dir}</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[var(--fg-muted)]">Expected Mods:</span>
              <span>{installedMods.length}</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[var(--fg-muted)]">Active PID:</span>
              <span>{session?.pid ? session.pid : "None"}</span>
            </div>
            <div className="flex justify-between items-center">
              <span className="text-[var(--fg-muted)]">Session State:</span>
              <span>{session?.state || "Inactive"}</span>
            </div>
            {logPath && (
              <div className="flex justify-between items-center border-t border-[var(--border)] pt-1 mt-1">
                <span className="text-[var(--fg-muted)]">Log File:</span>
                <span className="truncate max-w-md">{logPath}</span>
              </div>
            )}
          </div>

          {/* SMAPI Log Viewer */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between text-xs">
              <span className="font-semibold text-[var(--fg-primary)] flex items-center gap-1.5">
                SMAPI Console Log
                {isLoadingLogs && <span className="text-[var(--fg-muted)] text-[10px] animate-pulse">(updating...)</span>}
              </span>
              <div className="flex items-center gap-2">
                <button
                  onClick={fetchLogs}
                  disabled={isLoadingLogs}
                  className="px-2 py-1 bg-[var(--bg-elevated)] hover:bg-[var(--border)] border border-[var(--border)] rounded text-[11px] text-[var(--fg-primary)] cursor-pointer disabled:opacity-50"
                  title="Reload SMAPI log from disk"
                >
                  🔄 Refresh Logs
                </button>
                <button
                  onClick={handleCopyLogs}
                  disabled={!logContent}
                  className="px-2 py-1 bg-[var(--bg-elevated)] hover:bg-[var(--border)] border border-[var(--border)] rounded text-[11px] text-[var(--fg-primary)] cursor-pointer disabled:opacity-50"
                  title="Copy full log text to clipboard"
                >
                  {isCopied ? "✓ Copied!" : "📋 Copy Log"}
                </button>
              </div>
            </div>

            <div
              ref={logContainerRef}
              className="p-3 bg-black/95 text-xs font-mono rounded-lg max-h-72 overflow-y-auto select-text border border-neutral-800 space-y-0.5"
            >
              {logContent ? (
                logContent.split("\n").map((line, idx) => {
                  let colorClass = "text-neutral-300";
                  if (line.includes("ERROR") || line.includes("FAIL")) {
                    colorClass = "text-red-400 font-semibold";
                  } else if (line.includes("WARN")) {
                    colorClass = "text-amber-300";
                  } else if (line.includes("Loaded") || line.includes("SMAPI started")) {
                    colorClass = "text-green-400 font-semibold";
                  } else if (line.includes("TRACE")) {
                    colorClass = "text-neutral-500";
                  }

                  return (
                    <div key={idx} className={`${colorClass} leading-relaxed whitespace-pre-wrap break-all`}>
                      {line || "\u00A0"}
                    </div>
                  );
                })
              ) : (
                <div className="text-neutral-500 italic py-4 text-center">
                  {isLoadingLogs ? "Reading SMAPI log..." : "SMAPI log file is empty or has not been generated yet."}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </Card>
  );
};
