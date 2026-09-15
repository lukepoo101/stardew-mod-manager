import React, { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { GameInstallation } from "@/lib/backend/types";
import { backend } from "@/lib/backend/client";
import { api } from "@/shared/api/client";
import { Folder, RefreshCw } from "lucide-react";

export interface GameSelectionScreenProps {
  onGameSelected: (game: GameInstallation) => void;
  onCancel?: () => void;
  initialPath?: string;
}

export const GameSelectionScreen: React.FC<GameSelectionScreenProps> = ({
  onGameSelected,
  onCancel,
  initialPath = "",
}) => {
  const [candidates, setCandidates] = useState<GameInstallation[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [manualPath, setManualPath] = useState(initialPath);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadCandidates();
  }, []);

  const loadCandidates = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const found = await backend.discoverGames();
      setCandidates(found);
    } catch (e: any) {
      setError(e?.toString() || "Failed to discover game installations");
    } finally {
      setIsLoading(false);
    }
  };

  const handleManualChoose = async () => {
    if (!manualPath.trim()) return;
    setIsLoading(true);
    setError(null);
    try {
      const game = await backend.chooseGame(manualPath.trim());
      if (game.is_fresh || game.is_managed) {
        onGameSelected(game);
      } else {
        setCandidates([game]);
      }
    } catch (e: any) {
      setError(e?.toString() || "Failed to validate selected folder");
    } finally {
      setIsLoading(false);
    }
  };

  const handleBrowse = async () => {
    try {
      const folder = await api.pickFolderDialog();
      if (folder) {
        setManualPath(folder);
      }
    } catch (e: any) {
      setError(e?.toString() || "Failed to pick folder");
    }
  };

  return (
    <div className="max-w-2xl mx-auto space-y-6">
      {onCancel && (
        <div className="flex justify-start">
          <Button variant="ghost" size="md" onClick={onCancel}>
            ← Back to Mod Manager
          </Button>
        </div>
      )}

      <div className="text-center space-y-2">
        <h1 className="text-2xl font-extrabold text-[var(--fg-primary)] tracking-tight">
          Find your Stardew Valley
        </h1>
        <p className="text-[var(--fg-muted)] text-[15px]">
          We look for your native Steam installation on Linux. Fresh installations with no previous mods are supported.
        </p>
      </div>

      {error && (
        <div className="p-4 rounded-xl bg-[var(--danger-surface)] border border-[var(--danger)]/30 text-[var(--danger)] text-sm select-text">
          {error}
        </div>
      )}

      {isLoading ? (
        <Card className="text-center py-12">
          <div className="inline-block w-8 h-8 border-3 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin mb-3" />
          <p className="text-[var(--fg-muted)] text-sm">Scanning for Stardew Valley...</p>
        </Card>
      ) : candidates.length > 0 ? (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-semibold text-[var(--fg-muted)] uppercase tracking-wider">
              Discovered Installations ({candidates.length})
            </h3>
            <Button variant="ghost" size="sm" onClick={loadCandidates} disabled={isLoading}>
              <RefreshCw className="w-3.5 h-3.5 mr-1.5" />
              Scan Again
            </Button>
          </div>
          {candidates.map((game) => (
            <Card key={game.id} className="space-y-4 border-2 hover:border-[var(--border-focus)] transition-all">
              <div className="flex items-start justify-between">
                <div>
                  <div className="flex items-center gap-2 mb-1 flex-wrap">
                    <h3 className="font-bold text-lg text-[var(--fg-primary)]">
                      Stardew Valley
                    </h3>
                    <StatusBadge variant="info">
                      {game.platform_kind === "steam_native" ? "Steam Native" : "Manual Folder"}
                    </StatusBadge>
                    {game.detected_version && (
                      <span className="text-xs px-2 py-0.5 rounded-md bg-[var(--bg-elevated)] border border-[var(--border)] font-mono text-[var(--fg-primary)]">
                        v{game.detected_version}
                      </span>
                    )}
                    {game.is_managed ? (
                      <StatusBadge variant="success">Managed Game</StatusBadge>
                    ) : game.is_fresh ? (
                      <StatusBadge variant="success">Fresh Game</StatusBadge>
                    ) : (
                      <StatusBadge variant="danger">Existing Mods</StatusBadge>
                    )}
                  </div>
                  <p className="text-xs text-[var(--fg-muted)] font-mono break-all select-text">
                    {game.canonical_root}
                  </p>
                </div>
              </div>

              {!game.is_fresh && !game.is_managed ? (
                <div className="p-3 bg-[var(--danger-surface)] border border-[var(--danger)]/20 rounded-lg text-xs text-[var(--danger)] leading-relaxed select-text">
                  <strong>Non-fresh install:</strong> {game.validation_error || "Existing SMAPI or mods detected. The MVP requires a fresh unmodded installation."}
                </div>
              ) : null}

              <div className="flex items-center justify-end gap-3 pt-2">
                <Button
                  variant="primary"
                  size="lg"
                  disabled={!game.is_fresh && !game.is_managed}
                  onClick={() => onGameSelected(game)}
                >
                  Use this game
                </Button>
              </div>
            </Card>
          ))}
        </div>
      ) : (
        <Card className="text-center py-8 space-y-3">
          <p className="text-[var(--fg-primary)] font-medium">
            No Steam installations detected automatically
          </p>
          <p className="text-xs text-[var(--fg-muted)]">
            You can manually enter the directory where Stardew Valley is installed.
          </p>
          <div className="pt-2">
            <Button variant="ghost" size="sm" onClick={loadCandidates}>
              <RefreshCw className="w-3.5 h-3.5 mr-1.5" />
              Scan Again
            </Button>
          </div>
        </Card>
      )}

      {/* Manual folder selection */}
      <Card className="space-y-3">
        <h4 className="text-sm font-bold text-[var(--fg-primary)]">Choose game folder manually</h4>
        <div className="flex gap-2">
          <input
            type="text"
            placeholder="/home/.../steamapps/common/Stardew Valley"
            value={manualPath}
            onChange={(e) => setManualPath(e.target.value)}
            className="flex-1 px-3 py-2 bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg text-sm text-[var(--fg-primary)] focus:border-[var(--accent-primary)] outline-none font-mono"
          />
          <Button variant="secondary" onClick={handleBrowse} type="button">
            <Folder className="w-4 h-4 mr-1.5" />
            Browse
          </Button>
          <Button variant="primary" onClick={handleManualChoose} disabled={!manualPath.trim() || isLoading}>
            Validate folder
          </Button>
        </div>
      </Card>
    </div>
  );
};
