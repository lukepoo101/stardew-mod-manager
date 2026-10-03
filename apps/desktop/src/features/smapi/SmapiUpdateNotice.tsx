import React, { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveLaunchSession,
  useActiveProfileOverview,
} from "@/shared/api/hooks";
import { isRunningState } from "@/shared/launch/sessionResult";
import { SmapiSetup } from "./SmapiSetup";
import {
  useSaveSmapiUpdateSettings,
  useSmapiUpdateSettings,
} from "./SmapiPanel";

/**
 * Keeps SMAPI's release list current (read at most once a day) and acts on
 * a newer SMAPI that supports the game. By default it only tells; the first
 * notice also offers updating automatically from then on, or never being
 * told again. Automatic updates wait until the game is not running and
 * only install releases SMAPI published a checksum for.
 */
export const SmapiUpdateNotice: React.FC = () => {
  const client = useQueryClient();
  const { data: overview } = useActiveProfileOverview();
  const { data: session, isSuccess: sessionKnown } = useActiveLaunchSession();
  const { data: settings } = useSmapiUpdateSettings();
  const save = useSaveSmapiUpdateSettings();
  const [updating, setUpdating] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const attempted = useRef<string | null>(null);
  const checkedFor = useRef<string | null>(null);

  const gameId = overview?.game.id;
  const status = overview?.smapi_status;
  const managedGame = overview?.game.management_mode !== "external_unmanaged";
  const running = Boolean(session && isRunningState(session.state));
  const target = status?.recommended_version ?? null;

  // Reads SMAPI's release list once per game per run; the backend skips the
  // network when the list was read in the last day.
  useEffect(() => {
    if (!gameId || checkedFor.current === gameId) return;
    checkedFor.current = gameId;
    api
      .getSmapiCatalog(gameId)
      .then((catalog) => {
        client.setQueryData(["smapi-catalog", gameId], catalog);
      })
      .catch(() => undefined);
  }, [gameId, client]);

  const due = Boolean(
    gameId &&
      status &&
      settings &&
      managedGame &&
      status.update_available &&
      target &&
      settings.skipped_version !== target,
  );

  useEffect(() => {
    if (
      !due ||
      settings?.mode !== "auto" ||
      !sessionKnown ||
      running ||
      !gameId ||
      !target
    )
      return;
    if (attempted.current === target) return;
    attempted.current = target;
    const from = status?.observed_version;
    api
      .installPinnedSmapi(gameId, target)
      .then((after) => {
        setResult(
          after.state === "installed" && after.observed_version === target
            ? `SMAPI was updated automatically from ${from ?? "the previous version"} to ${target}. Try a modded launch to confirm your mods load; the previous installer is kept if you need to roll back.`
            : `An automatic update to SMAPI ${target} ran, but the game folder does not show it installed. See Activity.`,
        );
      })
      .catch((error) =>
        setResult(
          `SMAPI ${target} was not installed automatically: ${errorSummary(error)}. You can update from the SMAPI panel on the dashboard.`,
        ),
      );
  }, [
    due,
    settings?.mode,
    sessionKnown,
    running,
    gameId,
    target,
    status,
    client,
  ]);

  if (result)
    return (
      <div
        role="status"
        className="mx-6 mt-3 p-3 rounded-lg border border-[var(--border)] text-xs flex items-start justify-between gap-3"
      >
        <span>{result}</span>
        <button
          type="button"
          className="underline cursor-pointer shrink-0"
          onClick={() => setResult(null)}
        >
          Dismiss
        </button>
      </div>
    );
  if (!due || !settings || settings.mode !== "notify" || !gameId || !target)
    return null;

  const first = !settings.first_notice_seen;
  const choose = (patch: Partial<typeof settings>) =>
    void save({ ...settings, first_notice_seen: true, ...patch });

  return (
    <section
      aria-label="SMAPI update"
      className="mx-6 mt-3 p-3 rounded-lg border border-[var(--accent-primary)]/40 bg-[var(--bg-elevated)]/40 text-xs space-y-2"
    >
      <p>
        <span className="font-semibold">SMAPI {target} is available.</span> You
        have SMAPI {status?.observed_version ?? "(version unknown)"}. SMAPI says{" "}
        {target} supports{" "}
        {status?.game_version
          ? `Stardew Valley ${status.game_version}`
          : "your game"}
        .
        {first &&
          " You can update now, have the manager install updates like this by itself from now on, or not be told about SMAPI updates again. This can be changed in the SMAPI panel on the dashboard."}
      </p>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="primary"
          size="sm"
          disabled={running}
          title={running ? "Close the game first" : undefined}
          onClick={() => setUpdating(true)}
        >
          Update now...
        </Button>
        {first && (
          <Button
            variant="secondary"
            size="sm"
            onClick={() => choose({ mode: "auto" })}
          >
            Always update automatically
          </Button>
        )}
        <Button
          variant="ghost"
          size="sm"
          onClick={() => choose({ skipped_version: target })}
        >
          Skip this version
        </Button>
        {first && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => choose({ mode: "off" })}
          >
            Don't show this again
          </Button>
        )}
      </div>
      {updating && (
        <Modal
          labelledBy="smapi-update-title"
          onClose={() => setUpdating(false)}
          className="space-y-3 text-sm max-h-[85vh] overflow-y-auto"
        >
          <h2 id="smapi-update-title" className="text-lg font-bold">
            Update SMAPI to {target}?
          </h2>
          <SmapiSetup
            gameId={gameId}
            initialVersion={target}
            installedVersion={status?.observed_version}
            actionLabel="Update SMAPI"
            onInstalled={(after) => {
              setUpdating(false);
              if (first) choose({});
              setResult(
                after.state === "installed"
                  ? `SMAPI ${after.observed_version ?? ""} is installed, checked from the game folder. Try a modded launch to confirm your mods load.`
                  : "The installer ran but SMAPI is still not complete. See Activity.",
              );
            }}
            onBack={() => setUpdating(false)}
            backLabel="Cancel"
          />
        </Modal>
      )}
    </section>
  );
};
