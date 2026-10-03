import React, { useEffect, useRef, useState } from "react";
import { useLocation } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveLaunchSession,
  useActiveProfileOverview,
} from "@/shared/api/hooks";
import type {
  SmapiStatusDto,
  SmapiUpdateSettingsDto,
} from "@/shared/api/generated";
import { isRunningState } from "@/shared/launch/sessionResult";
import { smapiBadge, smapiExplanation } from "@/shared/smapi/status";
import { SmapiSetup } from "./SmapiSetup";

export const SMAPI_UPDATE_SETTINGS_KEY = ["smapi-update-settings"] as const;

export function useSmapiUpdateSettings() {
  return useQuery({
    queryKey: SMAPI_UPDATE_SETTINGS_KEY,
    queryFn: () => api.getSmapiUpdateSettings(),
    staleTime: 60_000,
  });
}

/** Saves update settings and refreshes everything that reads them. */
export function useSaveSmapiUpdateSettings() {
  const client = useQueryClient();
  return async (settings: SmapiUpdateSettingsDto) => {
    await api.setSmapiUpdateSettings(settings);
    client.setQueryData(SMAPI_UPDATE_SETTINGS_KEY, settings);
  };
}

interface Action {
  title: string;
  label: string;
  version?: string;
  intro: string;
}

function source(status: SmapiStatusDto): string {
  if (status.catalog_source === "builtin")
    return "Only the release this manager ships with is known; SMAPI's release list has not been read.";
  const when = status.catalog_checked_at
    ? new Date(status.catalog_checked_at).toLocaleString()
    : "an unknown time";
  return `From SMAPI's GitHub releases, checked ${when}.`;
}

/**
 * SMAPI for the active game: what is installed, whether it supports the
 * game as SMAPI declares, the suggested version, and every change the
 * manager can make to it, each previewed before anything happens.
 */
export const SmapiPanel: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const { data: session } = useActiveLaunchSession();
  const { data: settings } = useSmapiUpdateSettings();
  const save = useSaveSmapiUpdateSettings();
  const [action, setAction] = useState<Action | null>(null);
  const [removing, setRemoving] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const status = overview?.smapi_status;
  const { hash } = useLocation();
  const card = useRef<HTMLDivElement>(null);
  // The header's SMAPI badge links here.
  useEffect(() => {
    if (hash === "#smapi" && status) card.current?.scrollIntoView();
  }, [hash, status]);
  if (!status || !overview) return null;

  const gameId = overview.game.id;
  const managedGame = overview.game.management_mode !== "external_unmanaged";
  const running = Boolean(session && isRunningState(session.state));
  const installed = status.observed_version;
  const suggested = status.recommended_version;
  const rollbacks = (status.kept_versions ?? []).filter((v) => v !== installed);

  const actions: Action[] = [];
  if (status.state === "absent")
    actions.push({
      title: "Install SMAPI",
      label: "Install SMAPI",
      version: suggested ?? undefined,
      intro:
        "This runs SMAPI's own installer. Your mods and profiles are not part of it.",
    });
  else if (status.state === "partial")
    actions.push({
      title: "Repair SMAPI",
      label: "Repair SMAPI",
      version: installed ?? suggested ?? undefined,
      intro:
        "This runs SMAPI's own installer again over the incomplete files. It does not delete game files to repair them.",
    });
  if (status.state !== "absent" && status.update_available && suggested)
    actions.push({
      title: `Update SMAPI to ${suggested}`,
      label: "Update SMAPI",
      version: suggested,
      intro:
        "This runs SMAPI's own installer for the newer version. The current installer is kept so you can roll back.",
    });
  if (status.state === "installed" && installed)
    actions.push({
      title: status.managed
        ? `Reinstall SMAPI ${installed}`
        : `Let the manager look after SMAPI ${installed}`,
      label: "Reinstall SMAPI",
      version: installed,
      intro: status.managed
        ? "This runs the same version's installer again, which repairs changed SMAPI files."
        : "This reinstalls the same version from SMAPI's own download, so the manager knows exactly what is installed and can repair, update or roll it back.",
    });

  const close = () => setAction(null);
  const done = (after: SmapiStatusDto) => {
    setAction(null);
    setMessage(
      after.state === "installed"
        ? `SMAPI ${after.observed_version ?? ""} is in the game folder, checked from its files. Try a modded launch to confirm mods load.`
        : "The installer ran but SMAPI is still not complete. See Activity for details.",
    );
  };

  const remove = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const after = await api.uninstallSmapi(gameId);
      setMessage(
        after.is_installed
          ? "SMAPI is still in the game folder. See Activity for what happened."
          : "SMAPI was removed. Your mods and profiles are unchanged.",
      );
      setRemoving(false);
    } catch (error) {
      setMessage(errorSummary(error, "SMAPI was not removed"));
    } finally {
      setBusy(false);
    }
  };

  const badge = smapiBadge(status);
  return (
    <div id="smapi" ref={card}>
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
              : (installed ?? "Version unknown")}
            {status.state === "partial" ? " (incomplete)" : ""}
            {status.state !== "absent" &&
              (status.managed
                ? " · looked after by this manager"
                : " · not installed by this manager")}
          </dd>
          <dt className="text-[var(--fg-muted)]">Game</dt>
          <dd>
            {status.game_version
              ? `Stardew Valley ${status.game_version}`
              : "Version could not be read"}
          </dd>
          <dt className="text-[var(--fg-muted)]">Suggested</dt>
          <dd>
            {suggested
              ? `SMAPI ${suggested}`
              : "No known SMAPI supports this game"}
          </dd>
          <dt className="text-[var(--fg-muted)]">Releases</dt>
          <dd>{source(status)}</dd>
        </dl>
        <p className="text-xs">{smapiExplanation(status)}</p>
        {(status.evidence ?? []).length > 0 && (
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
        {!managedGame ? (
          <p className="text-xs text-[var(--fg-muted)]">
            This installation is not managed, so SMAPI is not changed from here.
          </p>
        ) : running ? (
          <p className="text-xs text-[var(--fg-muted)]">
            SMAPI cannot be changed while the game is running.
          </p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {actions.map((a, index) => (
              <Button
                key={a.title}
                variant={index === 0 ? "primary" : "secondary"}
                size="sm"
                onClick={() => setAction(a)}
              >
                {a.title}...
              </Button>
            ))}
            <Button
              variant="secondary"
              size="sm"
              onClick={() =>
                setAction({
                  title: "Change SMAPI version",
                  label: "Install this version",
                  version: installed ?? suggested ?? undefined,
                  intro:
                    "Choose any SMAPI release. Versions that support your game are listed first; older installers kept here work without the network.",
                })
              }
            >
              Change version...
            </Button>
            {rollbacks.map((version) => (
              <Button
                key={version}
                variant="ghost"
                size="sm"
                onClick={() =>
                  setAction({
                    title: `Roll back to SMAPI ${version}`,
                    label: "Roll back SMAPI",
                    version,
                    intro: `SMAPI ${version} was installed here before and its verified installer is kept, so this works without the network.`,
                  })
                }
              >
                Roll back to {version}...
              </Button>
            ))}
            {status.state !== "absent" && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setRemoving(true)}
              >
                Remove SMAPI...
              </Button>
            )}
          </div>
        )}
        {settings && (
          <label className="flex flex-wrap items-center gap-2 text-xs pt-1">
            <span>When a newer SMAPI supports your game:</span>
            <select
              aria-label="SMAPI updates"
              value={settings.mode}
              onChange={(e) =>
                void save({
                  ...settings,
                  mode: e.target.value,
                  first_notice_seen: true,
                })
              }
              className="px-2 py-1 bg-[var(--bg-primary)] border border-[var(--border)] rounded-md"
            >
              <option value="notify">Tell me</option>
              <option value="auto">Update automatically</option>
              <option value="off">Do nothing</option>
            </select>
          </label>
        )}
        {action && (
          <Modal
            labelledBy="smapi-action-title"
            onClose={busy ? undefined : close}
            className="space-y-3 text-sm max-h-[85vh] overflow-y-auto"
          >
            <h2 id="smapi-action-title" className="text-lg font-bold">
              {action.title}?
            </h2>
            <p className="text-xs">
              {action.intro} Nothing changes until you confirm, and the game
              folder is checked afterwards.
            </p>
            <SmapiSetup
              gameId={gameId}
              initialVersion={action.version}
              installedVersion={installed}
              actionLabel={action.label}
              onInstalled={done}
              onBack={close}
              backLabel="Cancel"
            />
          </Modal>
        )}
        {removing && (
          <Modal
            labelledBy="remove-smapi-title"
            onClose={busy ? undefined : () => setRemoving(false)}
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
                SMAPI {installed ?? "(version unknown)"} is removed by its own
                installer's uninstall mode, which changes files in the game
                folder.
              </li>
              <li>
                Your profiles, their mods and stored packages are kept. Mods
                will not load until SMAPI is installed again, and modded
                launches are unavailable until then.
              </li>
              <li>
                The manager checks the game folder afterwards. There is no
                automatic undo; installing SMAPI again puts it back.
              </li>
            </ul>
            <div className="flex justify-end gap-2">
              <Button
                variant="secondary"
                disabled={busy}
                onClick={() => setRemoving(false)}
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
    </div>
  );
};
