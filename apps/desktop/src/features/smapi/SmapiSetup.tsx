import React, { useCallback, useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { SetupPreviewDto, SmapiStatusDto } from "@/shared/api/generated";
import { SetupPreview } from "@/features/onboarding/SetupPreview";
import { compareVersions } from "@/shared/versions";
import { SmapiVersionPicker } from "./SmapiVersionPicker";

/**
 * Installing SMAPI at a version: the suggested one unless another is chosen,
 * with the backend's preview of exactly what will change. A version that
 * does not support the game, or that was published without a checksum,
 * needs an explicit yes. Nothing changes until the install button.
 */
export const SmapiSetup: React.FC<{
  gameId: string;
  /** The version to start with; the suggested one when absent. */
  initialVersion?: string;
  /** SMAPI already in the game folder, to say when a choice is older. */
  installedVersion?: string | null;
  actionLabel?: string;
  onInstalled: (status: SmapiStatusDto) => void;
  onBack?: () => void;
  backLabel?: string;
}> = ({
  gameId,
  initialVersion,
  installedVersion,
  actionLabel = "Install SMAPI",
  onInstalled,
  onBack,
  backLabel = "Back",
}) => {
  const [version, setVersion] = useState<string | undefined>(initialVersion);
  const [picking, setPicking] = useState(false);
  const [preview, setPreview] = useState<SetupPreviewDto | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [allowUnverified, setAllowUnverified] = useState(false);
  const [allowUnsupported, setAllowUnsupported] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const onReady = useCallback((p: SetupPreviewDto | null) => {
    setPreview(p);
    setAllowUnverified(false);
    setAllowUnsupported(false);
  }, []);

  const unsupported =
    preview?.compatibility === "game_too_old" ||
    preview?.compatibility === "game_too_new";
  const older =
    preview && installedVersion
      ? compareVersions(preview.smapi_version, installedVersion) < 0
      : false;
  const ready =
    Boolean(preview?.can_proceed) &&
    (preview?.checksum_published || allowUnverified) &&
    (!unsupported || allowUnsupported);

  const install = async () => {
    if (!preview) return;
    setBusy(true);
    setError(null);
    try {
      onInstalled(
        await api.installPinnedSmapi(gameId, preview.smapi_version, {
          allowUnverified,
        }),
      );
    } catch (installError) {
      const code = (installError as { code?: string }).code;
      if (code === "SETUP_PLAN_CHANGED" || code === "SETUP_CHECKS_FAILED")
        setAttempt((n) => n + 1);
      setError(errorSummary(installError, "SMAPI was not changed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3 text-xs">
      <SetupPreview
        gameId={gameId}
        version={version}
        attempt={attempt}
        onReady={onReady}
      />
      <button
        type="button"
        className="underline cursor-pointer"
        onClick={() => setPicking((p) => !p)}
      >
        {picking ? "Hide the version list" : "Choose another version..."}
      </button>
      {picking && (
        <SmapiVersionPicker
          gameId={gameId}
          selected={version ?? preview?.smapi_version}
          onSelect={(v) => setVersion(v)}
        />
      )}
      {older && (
        <p className="text-[var(--warning)]">
          SMAPI {preview?.smapi_version} is older than the SMAPI{" "}
          {installedVersion} in the game folder. Mods that need the newer one
          may stop loading.
        </p>
      )}
      {preview && unsupported && (
        <label className="flex items-start gap-2 text-[var(--warning)]">
          <input
            type="checkbox"
            checked={allowUnsupported}
            onChange={(e) => setAllowUnsupported(e.target.checked)}
          />
          <span>
            Install it anyway, although SMAPI says it does not support this game
            version. SMAPI may refuse to start.
          </span>
        </label>
      )}
      {preview && !preview.checksum_published && (
        <label className="flex items-start gap-2 text-[var(--warning)]">
          <input
            type="checkbox"
            checked={allowUnverified}
            onChange={(e) => setAllowUnverified(e.target.checked)}
          />
          <span>
            Install it without a published checksum. The download comes from
            SMAPI's GitHub page over a secure connection, but it cannot be
            checked against a published value. Its checksum is recorded here for
            next time.
          </span>
        </label>
      )}
      {busy && (
        <p role="status" className="flex items-center gap-2">
          <span className="w-4 h-4 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full animate-spin" />
          Installing SMAPI and verifying installation files...
        </p>
      )}
      {error && (
        <p role="alert" className="text-[var(--danger)]">
          {error}
        </p>
      )}
      <div className="flex justify-between items-center pt-1">
        {onBack ? (
          <Button variant="ghost" onClick={onBack} disabled={busy}>
            {backLabel}
          </Button>
        ) : (
          <span />
        )}
        <Button
          variant="primary"
          onClick={() => void install()}
          isLoading={busy}
          disabled={busy || !ready}
        >
          {actionLabel}
        </Button>
      </div>
    </div>
  );
};
