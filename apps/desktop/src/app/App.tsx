import React, { useEffect, useState } from "react";
import { GameSelectionScreen } from "@/features/setup/GameSelectionScreen";
import { SmapiSetupScreen } from "@/features/setup/SmapiSetupScreen";
import { ModDropZone } from "@/features/mods/ModDropZone";
import { ModReviewDialog } from "@/features/mods/ModReviewDialog";
import { ModList } from "@/features/mods/ModList";
import { LaunchPanel } from "@/features/launch/LaunchPanel";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { AppSnapshot, ArchiveInspectionResult } from "@/lib/backend/types";
import { backend } from "@/lib/backend/client";

export const App: React.FC = () => {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const [activeInspection, setActiveInspection] = useState<ArchiveInspectionResult | null>(null);
  const [isChangingFolder, setIsChangingFolder] = useState(false);

  useEffect(() => {
    // Detect system preference
    if (window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches) {
      setTheme("dark");
      document.documentElement.classList.add("dark");
    }
    loadSnapshot();
  }, []);

  const toggleTheme = () => {
    const next = theme === "light" ? "dark" : "light";
    setTheme(next);
    if (next === "dark") {
      document.documentElement.classList.add("dark");
    } else {
      document.documentElement.classList.remove("dark");
    }
  };

  const loadSnapshot = async () => {
    setIsLoading(true);
    try {
      const snap = await backend.getAppSnapshot();
      setSnapshot(snap);
    } catch (e) {
      console.error("Failed to load snapshot:", e);
    } finally {
      setIsLoading(false);
    }
  };

  if (isLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-[var(--bg-primary)]">
        <div className="text-center space-y-3">
          <div className="inline-block w-8 h-8 border-3 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin" />
          <p className="text-sm font-medium text-[var(--fg-muted)]">Loading Stardew Mod Manager...</p>
        </div>
      </div>
    );
  }

  if (snapshot?.recovery_error || snapshot?.active_operation) {
    return <main className="p-8 space-y-4"><h1>Recovery required</h1>
      <p role="alert">{snapshot.recovery_error || "An interrupted operation needs recovery."}</p>
      <p>Exit the game before retrying. Your recovery journal is preserved.</p>
      <button onClick={async () => { try { await backend.retryRecovery(); } finally { await loadSnapshot(); } }}>Retry recovery</button>
    </main>;
  }

  // View routing based on installation state
  const renderContent = () => {
    if (!snapshot?.selected_game || isChangingFolder) {
      return (
        <GameSelectionScreen
          initialPath={snapshot?.selected_game?.canonical_root}
          onCancel={snapshot?.selected_game ? () => setIsChangingFolder(false) : undefined}
          onGameSelected={async (game) => {
            setIsLoading(true);
            try {
              const snap = await backend.selectGame(game.canonical_root, game.platform_kind);
              setSnapshot(snap);
              setIsChangingFolder(false);
            } catch (err) {
              console.error("Failed to select game:", err);
              await loadSnapshot();
            } finally {
              setIsLoading(false);
            }
          }}
        />
      );
    }

    if (!snapshot.smapi_installed) {
      return (
        <SmapiSetupScreen
          game={snapshot.selected_game}
          onSmapiInstalled={async () => {
            await loadSnapshot();
          }}
        />
      );
    }

    return (
      <div className="space-y-6">
        {snapshot.setup && (
          <LaunchPanel
            game={snapshot.selected_game}
            setup={snapshot.setup}
            installedMods={snapshot.installed_mods}
            initialSession={snapshot.active_session}
          />
        )}

        {snapshot.setup && (
          <ModDropZone
            setupId={snapshot.setup.id}
            onInspectionReady={(res) => setActiveInspection(res)}
          />
        )}

        {snapshot.setup && (
          <ModList
            mods={snapshot.installed_mods}
            setupId={snapshot.setup.id}
            onModRemoved={() => loadSnapshot()}
          />
        )}

        <ModReviewDialog
          inspection={activeInspection}
          onClose={() => setActiveInspection(null)}
          onModInstalled={() => loadSnapshot()}
        />
      </div>
    );
  };

  return (
    <div className="min-h-screen bg-[var(--bg-primary)] text-[var(--fg-primary)] flex flex-col">
      {/* Top Header */}
      <header className="border-b border-[var(--border)] bg-[var(--bg-surface)] px-6 py-4 flex items-center justify-between sticky top-0 z-10 shadow-xs">
        <div className="flex items-center gap-3">
          <span className="text-2xl">🌾</span>
          <div>
            <h1 className="font-extrabold text-base tracking-tight">Stardew Mod Manager</h1>
            {snapshot?.selected_game && (
              <div className="flex items-center gap-2">
                <p className="text-[11px] text-[var(--fg-muted)] font-mono truncate max-w-xs sm:max-w-md select-text">
                  {snapshot.selected_game.canonical_root}
                </p>
                {!isChangingFolder && (
                  <button
                    onClick={() => setIsChangingFolder(true)}
                    className="text-[11px] text-[var(--accent-primary)] hover:underline cursor-pointer font-medium whitespace-nowrap"
                    title="Change game folder"
                  >
                    Change Folder
                  </button>
                )}
              </div>
            )}
          </div>
        </div>

        <div className="flex items-center gap-3">
          {snapshot?.smapi_installed && (
            <StatusBadge variant="success">SMAPI 4.1.10 Ready</StatusBadge>
          )}

          <button
            onClick={toggleTheme}
            className="p-2 rounded-lg hover:bg-[var(--bg-elevated)] border border-[var(--border)] text-sm cursor-pointer"
            aria-label="Toggle theme"
          >
            {theme === "light" ? "🌙" : "☀️"}
          </button>
        </div>
      </header>

      {/* Main Container */}
      <main className="flex-1 max-w-4xl w-full mx-auto p-6 md:p-8">
        {renderContent()}
      </main>

      {/* Footer */}
      <footer className="border-t border-[var(--border)] px-6 py-3 text-center text-xs text-[var(--fg-muted)]">
        Stardew Mod Manager • Linux Edition • MIT License
      </footer>
    </div>
  );
};
