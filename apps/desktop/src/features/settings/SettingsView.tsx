import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { useTheme } from "@/shared/theme/ThemeProvider";
import {
  useGameInstallations,
  useActiveProfileOverview,
  useDiscoverGames,
} from "@/shared/api/hooks";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "@/shared/api/client";
import {
  Folder,
  Palette,
  Info,
  Sun,
  Moon,
  Laptop,
  RefreshCw,
} from "lucide-react";

export const SettingsView: React.FC = () => {
  const cache = useQueryClient();
  const { theme, setTheme } = useTheme();
  const { data: games, refetch: refetchGames } = useGameInstallations();
  const {
    data: discoveredGames,
    isFetching: isDiscovering,
    error: discoveryError,
    refetch: refetchDiscovered,
  } = useDiscoverGames();
  const { data: overview } = useActiveProfileOverview();

  const [newGamePath, setNewGamePath] = useState("");
  const [isRegistering, setIsRegistering] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleBrowseFolder = async () => {
    try {
      const folder = await api.pickFolderDialog();
      if (folder) {
        setNewGamePath(folder);
      }
    } catch (error) {
      setError(String(error));
    }
  };

  const handleRegisterDiscovered = async (path: string, storefront: string) => {
    setIsRegistering(true);
    setError(null);
    try {
      await api.registerGameInstallation(path, storefront);
      cache.invalidateQueries();
      await refetchGames();
      await refetchDiscovered();
    } catch (e: any) {
      setError(e?.message || String(e));
    } finally {
      setIsRegistering(false);
    }
  };

  const handleRegisterGame = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newGamePath.trim()) return;
    setIsRegistering(true);
    setError(null);
    try {
      await api.registerGameInstallation(newGamePath.trim(), "Manual");
      cache.invalidateQueries();
      setNewGamePath("");
      refetchGames();
      refetchDiscovered();
    } catch (e: any) {
      setError(e?.message || String(e));
    } finally {
      setIsRegistering(false);
    }
  };

  return (
    <div className="max-w-4xl space-y-6">
      <div>
        <h2 className="text-xl font-bold tracking-tight">Settings</h2>
        <p className="text-sm text-[var(--fg-muted)]">
          Manage appearance, game installations, and storage configuration.
        </p>
      </div>

      {(error || discoveryError) && (
        <div className="p-4 rounded-xl bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-[var(--danger)] text-sm">
          {error || discoveryError?.message}
        </div>
      )}

      {/* Appearance */}
      <Card className="space-y-4">
        <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
          <Palette className="w-4 h-4 text-[var(--accent-primary)]" />
          <h3 className="font-bold text-sm">Appearance & Theme</h3>
        </div>

        <div className="grid grid-cols-3 gap-3">
          <button
            onClick={() => setTheme("system")}
            className={`p-4 rounded-xl border flex flex-col items-center gap-2 cursor-pointer transition-all ${
              theme === "system"
                ? "border-[var(--accent-primary)] bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] font-semibold"
                : "border-[var(--border)] hover:bg-[var(--bg-elevated)] text-[var(--fg-muted)]"
            }`}
          >
            <Laptop className="w-5 h-5" />
            <span className="text-xs">System</span>
          </button>

          <button
            onClick={() => setTheme("light")}
            className={`p-4 rounded-xl border flex flex-col items-center gap-2 cursor-pointer transition-all ${
              theme === "light"
                ? "border-[var(--accent-primary)] bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] font-semibold"
                : "border-[var(--border)] hover:bg-[var(--bg-elevated)] text-[var(--fg-muted)]"
            }`}
          >
            <Sun className="w-5 h-5" />
            <span className="text-xs">Light</span>
          </button>

          <button
            onClick={() => setTheme("dark")}
            className={`p-4 rounded-xl border flex flex-col items-center gap-2 cursor-pointer transition-all ${
              theme === "dark"
                ? "border-[var(--accent-primary)] bg-[var(--accent-primary)]/10 text-[var(--accent-primary)] font-semibold"
                : "border-[var(--border)] hover:bg-[var(--bg-elevated)] text-[var(--fg-muted)]"
            }`}
          >
            <Moon className="w-5 h-5" />
            <span className="text-xs">Dark</span>
          </button>
        </div>
      </Card>

      {/* Game Installations */}
      <Card className="space-y-4">
        <div className="flex items-center justify-between border-b border-[var(--border)] pb-3">
          <div className="flex items-center gap-2">
            <Folder className="w-4 h-4 text-amber-500" />
            <h3 className="font-bold text-sm">Registered Game Installations</h3>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => refetchDiscovered()}
            disabled={isDiscovering}
          >
            <RefreshCw
              className={`w-3.5 h-3.5 mr-1.5 ${isDiscovering ? "animate-spin" : ""}`}
            />
            Scan for Steam
          </Button>
        </div>

        <div className="space-y-2">
          {games?.map((game) => (
            <div
              key={game.id}
              className="p-3 rounded-lg border border-[var(--border)] bg-[var(--bg-elevated)]/20 flex items-center justify-between gap-3 text-xs"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2 mb-0.5">
                  <span className="font-bold text-[var(--fg-primary)]">
                    {game.storefront} ({game.operating_system})
                  </span>
                  {game.id === overview?.game.id && (
                    <StatusBadge variant="success">Active</StatusBadge>
                  )}
                </div>
                <p className="font-mono text-[var(--fg-muted)] truncate select-text">
                  {game.canonical_root}
                </p>
              </div>
            </div>
          ))}
        </div>

        {/* Discovered Unregistered Games */}
        {discoveredGames &&
          discoveredGames.filter(
            (d) => !games?.some((g) => g.canonical_root === d.candidate_path),
          ).length > 0 && (
            <div className="pt-2 border-t border-[var(--border)] space-y-2">
              <h4 className="text-xs font-semibold text-[var(--fg-muted)] uppercase tracking-wider">
                Discovered On Your System
              </h4>
              {discoveredGames
                .filter(
                  (d) =>
                    !games?.some((g) => g.canonical_root === d.candidate_path),
                )
                .map((d) => (
                  <div
                    key={d.candidate_path}
                    className="p-3 rounded-lg border border-dashed border-[var(--accent-primary)]/40 bg-[var(--accent-primary)]/5 flex items-center justify-between gap-3 text-xs"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2 mb-0.5">
                        <span className="font-bold text-[var(--fg-primary)]">
                          {d.storefront === "steam"
                            ? "Steam Native"
                            : d.storefront}
                        </span>
                        {d.detected_version && (
                          <span className="text-[11px] px-1.5 py-0.5 rounded bg-[var(--bg-elevated)] border border-[var(--border)] font-mono">
                            v{d.detected_version}
                          </span>
                        )}
                        <StatusBadge variant={d.is_usable ? "info" : "danger"}>
                          {d.is_usable ? "Detected" : "Unsupported"}
                        </StatusBadge>
                      </div>
                      <p className="font-mono text-[var(--fg-muted)] truncate select-text">
                        {d.candidate_path}
                      </p>
                    </div>
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={!d.is_usable || isRegistering}
                      onClick={() =>
                        handleRegisterDiscovered(d.candidate_path, d.storefront)
                      }
                    >
                      + Add to Manager
                    </Button>
                  </div>
                ))}
            </div>
          )}

        {/* Add game */}
        <form
          onSubmit={handleRegisterGame}
          className="flex gap-2 pt-2 border-t border-[var(--border)]"
        >
          <input
            type="text"
            placeholder="Add Stardew Valley directory path..."
            value={newGamePath}
            onChange={(e) => setNewGamePath(e.target.value)}
            className="flex-1 px-3 py-2 bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg text-sm text-[var(--fg-primary)] focus:border-[var(--accent-primary)] outline-none font-mono"
          />
          <Button
            variant="secondary"
            type="button"
            onClick={handleBrowseFolder}
          >
            Browse
          </Button>
          <Button
            variant="primary"
            type="submit"
            disabled={!newGamePath.trim() || isRegistering}
            isLoading={isRegistering}
          >
            Register
          </Button>
        </form>
      </Card>

      {/* About */}
      <Card className="space-y-3">
        <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
          <Info className="w-4 h-4 text-[var(--fg-muted)]" />
          <h3 className="font-bold text-sm">About Stardew Mod Manager</h3>
        </div>
        <div className="text-xs text-[var(--fg-muted)] space-y-1">
          <p>Version 0.1.0 (Architecture Foundation Vertical Slice)</p>
          <p>
            Strict Modular Monolith: Rust backend + SQLite + Tauri 2 + React
          </p>
          <p>MIT Licensed • Designed for Steam and custom game setups</p>
        </div>
      </Card>
    </div>
  );
};
