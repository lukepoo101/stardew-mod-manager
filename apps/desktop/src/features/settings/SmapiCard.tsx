import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useActiveProfileOverview } from "@/shared/api/hooks";
import type { SetupPreviewDto, SmapiStatusDto } from "@/shared/api/generated";
import { SetupPreview } from "@/features/onboarding/SetupPreview";

/**
 * What re-running the tested installer would do for this state, if it is
 * offered at all. A newer SMAPI is never replaced from here.
 */
function setupAction(status: SmapiStatusDto): string | null {
  if (status.state === "absent") return "Install SMAPI";
  if (status.state === "partial") return "Repair SMAPI";
  if (status.comparison === "older")
    return `Update SMAPI to ${status.tested_version}`;
  if (status.comparison === "same") return "Reinstall SMAPI";
  return null;
}
import { smapiBadge, smapiExplanation } from "@/shared/smapi/status";

/**
 * The SMAPI in the active game folder, next to the version this manager is
 * tested with. These are separate facts; neither is the "latest" release.
 */
export const SmapiCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const status = overview?.smapi_status;
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [setupOpen, setSetupOpen] = useState(false);
  const [preview, setPreview] = useState<SetupPreviewDto | null>(null);
  const [attempt, setAttempt] = useState(0);
  if (!status || !overview) return null;
  const action = setupAction(status);
  const runSetup = async () => {
    if (!preview) return;
    setBusy(true);
    setMessage(null);
    try {
      const after = await api.installPinnedSmapi(
        overview.game.id,
        preview.smapi_version,
      );
      setMessage(
        after.state === "installed"
          ? `SMAPI ${after.observed_version ?? ""} is in the game folder, checked from its files.`
          : "The installer ran but SMAPI is still not complete. See Activity for details.",
      );
      setSetupOpen(false);
    } catch (error) {
      const code = (error as { code?: string }).code;
      if (code === "SETUP_PLAN_CHANGED" || code === "SETUP_CHECKS_FAILED") {
        setAttempt((n) => n + 1);
      }
      setMessage(errorSummary(error, "SMAPI was not changed"));
    } finally {
      setBusy(false);
    }
  };
  const managed = overview.game.management_mode !== "external_unmanaged";
  const remove = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const after = await api.uninstallSmapi(overview.game.id);
      setMessage(
        after.is_installed
          ? "SMAPI is still in the game folder. See Activity for what happened."
          : "SMAPI was removed. Your mods and profiles are unchanged.",
      );
      setConfirming(false);
    } catch (error) {
      setMessage(errorSummary(error, "SMAPI was not removed"));
    } finally {
      setBusy(false);
    }
  };
  const badge = smapiBadge(status);
  return (
    <Card className="space-y-3 text-sm">
      <div className="flex items-center justify-between">
        <h3 className="font-bold">SMAPI</h3>
        <StatusBadge variant={badge.variant}>{badge.label}</StatusBadge>
      </div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs">
        <dt className="text-[var(--fg-muted)]">Installed</dt>
        <dd>
          {status.state === "absent"
            ? "None"
            : (status.observed_version ?? "Version unknown")}
          {status.state === "partial" ? " (incomplete)" : ""}
        </dd>
        <dt className="text-[var(--fg-muted)]">Tested with this manager</dt>
        <dd>{status.tested_version}</dd>
        <dt className="text-[var(--fg-muted)]">Latest release</dt>
        <dd>Not checked. No update source is connected.</dd>
      </dl>
      <p className="text-xs">{smapiExplanation(status)}</p>
      {status.evidence.length > 0 && (
        <details className="text-xs text-[var(--fg-muted)]">
          <summary className="cursor-pointer">What was checked</summary>
          <ul className="list-disc pl-4">
            {status.evidence.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </details>
      )}
      {message && (
        <p role="status" className="text-xs">
          {message}
        </p>
      )}
      {managed && action && (
        <Button
          variant="secondary"
          size="sm"
          onClick={() => setSetupOpen(true)}
        >
          {action}...
        </Button>
      )}
      {setupOpen && action && (
        <Modal
          labelledBy="smapi-setup-title"
          onClose={busy ? undefined : () => setSetupOpen(false)}
          className="space-y-3 text-sm"
        >
          <h2 id="smapi-setup-title" className="text-lg font-bold">
            {action}?
          </h2>
          <p className="text-xs">
            This runs SMAPI's own installer for the tested release. It does not
            delete game files to repair them, and your mods and profiles are not
            part of it. There is no automatic undo.
          </p>
          <SetupPreview
            gameId={overview.game.id}
            attempt={attempt}
            onReady={setPreview}
          />
          <div className="flex justify-end gap-2">
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => setSetupOpen(false)}
            >
              Cancel
            </Button>
            <Button
              variant="primary"
              isLoading={busy}
              disabled={busy || !preview?.can_proceed}
              onClick={runSetup}
            >
              {action}
            </Button>
          </div>
        </Modal>
      )}
      {managed && status.state !== "absent" && (
        <button
          type="button"
          className="text-xs underline text-[var(--fg-muted)] cursor-pointer"
          onClick={() => setConfirming(true)}
        >
          Remove SMAPI...
        </button>
      )}
      {confirming && (
        <Modal
          labelledBy="remove-smapi-title"
          onClose={busy ? undefined : () => setConfirming(false)}
          className="space-y-3 text-sm"
        >
          <h2 id="remove-smapi-title" className="text-lg font-bold">
            Remove SMAPI from the game?
          </h2>
          <p className="font-mono text-xs break-all">
            {overview.game.canonical_root}
          </p>
          <ul className="list-disc pl-4 text-xs space-y-1">
            <li>
              SMAPI {status.observed_version ?? "(version unknown)"} is removed
              by its own installer's uninstall mode, which changes files in the
              game folder.
            </li>
            <li>
              Your profiles, their mods and stored packages are kept. Mods will
              not load until SMAPI is installed again, and modded launches are
              unavailable until then.
            </li>
            <li>
              The manager checks the game folder afterwards. There is no
              automatic undo; setting SMAPI up again reinstalls it.
            </li>
          </ul>
          <div className="flex justify-end gap-2">
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => setConfirming(false)}
            >
              Cancel
            </Button>
            <Button variant="danger" isLoading={busy} onClick={remove}>
              Remove SMAPI
            </Button>
          </div>
        </Modal>
      )}
    </Card>
  );
};
