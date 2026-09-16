import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { GameInstallation } from "@/lib/backend/types";
import { backend } from "@/lib/backend/client";

export interface SmapiSetupScreenProps {
  game: GameInstallation;
  onSmapiInstalled: () => void;
}

export const SmapiSetupScreen: React.FC<SmapiSetupScreenProps> = ({
  game,
  onSmapiInstalled,
}) => {
  const [isInstalling, setIsInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleInstall = async () => {
    setIsInstalling(true);
    setError(null);

    try {
      await backend.installSmapi(game.id);
      onSmapiInstalled();
    } catch (e: any) {
      setError(e?.toString() || "Failed to install SMAPI");
      setIsInstalling(false);
    }
  };

  return (
    <div className="max-w-xl mx-auto space-y-6">
      <div className="text-center space-y-2">
        <h1 className="text-2xl font-extrabold text-[var(--fg-primary)] tracking-tight">
          Set up modding
        </h1>
        <p className="text-[var(--fg-muted)] text-[15px] leading-relaxed">
          SMAPI is the open-source mod loader required to load and run mods in
          Stardew Valley.
        </p>
      </div>

      <Card className="space-y-5">
        <div className="flex items-center justify-between border-b border-[var(--border)] pb-4">
          <div>
            <h3 className="font-bold text-[var(--fg-primary)]">SMAPI 4.1.10</h3>
            <p className="text-xs text-[var(--fg-muted)]">
              Official release for Stardew Valley 1.6.9+
            </p>
          </div>
          <StatusBadge variant="info">Verified Release</StatusBadge>
        </div>

        <div className="space-y-2 text-xs text-[var(--fg-muted)] font-mono bg-[var(--bg-primary)] p-3 rounded-lg border border-[var(--border)]">
          <div>
            SHA-256:
            8c127148a76c890e485aea73910189dc41c80f822c2a0a5e3b0e762b4ee3a93e
          </div>
          <div>Target: {game.canonical_root}</div>
        </div>

        {isInstalling && (
          <p role="status">
            Installing SMAPI and checking installed files. This may take a few
            minutes.
          </p>
        )}

        {error && (
          <div className="p-3 bg-[var(--danger-surface)] border border-[var(--danger)]/30 rounded-lg text-xs text-[var(--danger)] leading-relaxed select-text">
            <strong>Installation error:</strong> {error}
          </div>
        )}

        <div className="flex justify-end pt-2">
          <Button
            variant="primary"
            size="lg"
            isLoading={isInstalling}
            disabled={isInstalling}
            onClick={handleInstall}
            className="w-full sm:w-auto"
          >
            Install SMAPI
          </Button>
        </div>
      </Card>
    </div>
  );
};
