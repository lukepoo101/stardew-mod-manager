import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useLatestLaunchSession,
  useProfileFreeze,
  useProfileMods,
} from "@/shared/api/hooks";
import {
  freezeDrift,
  sameMods,
  settingsDrift,
} from "@/shared/profiles/freezeDrift";
import { useQuery } from "@tanstack/react-query";
import { Snowflake } from "lucide-react";

/**
 * Freezes the active profile at its current mods and versions, for example
 * before a multiplayer session. While frozen nothing can be installed or
 * removed; enabling and disabling stay possible and are shown as drift.
 */
export const FreezeCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const { data: freeze } = useProfileFreeze(profileId);
  const { data: mods } = useProfileMods(profileId);
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { data: latest } = useLatestLaunchSession();
  const { data: hashes } = useQuery({
    queryKey: ["settings-hashes", profileId],
    queryFn: () => api.settingsHashes(profileId ?? ""),
    enabled: Boolean(profileId && freeze),
  });
  const { data: knownGood } = useQuery({
    queryKey: ["known-good", profileId],
    queryFn: () => api.getKnownGood(profileId ?? ""),
    enabled: Boolean(profileId && freeze),
  });

  if (!profileId) return null;

  /**
   * Saves a restore point first (it keeps the mods' settings and outlives
   * the freeze), then freezes with the versions last observed.
   */
  const freezeNow = () =>
    run(async () => {
      const label = reason.trim()
        ? `Frozen: ${reason.trim()}`.slice(0, 80)
        : `Frozen ${new Date().toLocaleDateString()}`;
      await api.createRestorePoint(profileId, label);
      await api.freezeProfile(profileId, reason, {
        gameVersion:
          latest?.profile_id === profileId ? latest.game_version : null,
        smapiVersion: overview?.smapi_status?.observed_version ?? null,
      });
    });

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      setReason("");
    } catch (actionError) {
      setError(errorSummary(actionError, "That did not work"));
    } finally {
      setBusy(false);
    }
  };

  const drift = freeze && mods ? freezeDrift(freeze.mods, mods) : null;
  // Freezes made before settings were recorded have none to compare with.
  const settingsChanged =
    freeze && hashes && freeze.settings.length > 0
      ? settingsDrift(freeze.settings, hashes)
      : [];
  const nameOf = (id: string) =>
    mods?.find((m) => m.unique_id.toLowerCase() === id)?.name ?? id;
  const drifted =
    drift &&
    drift.enabledChanged.length +
      drift.added.length +
      drift.removed.length +
      settingsChanged.length >
      0;

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Snowflake className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Freeze this profile</h3>
        {freeze && <StatusBadge variant="info">Frozen</StatusBadge>}
      </div>
      {freeze ? (
        <div className="text-xs space-y-2">
          <p>
            Frozen {new Date(freeze.frozen_at).toLocaleString()} with{" "}
            {freeze.mods.length} mod(s)
            {freeze.reason ? `: ${freeze.reason}` : "."}
            {freeze.game_version || freeze.smapi_version
              ? ` Last seen with Stardew Valley ${freeze.game_version ?? "(unknown)"}, SMAPI ${freeze.smapi_version ?? "(unknown)"}.`
              : ""}
          </p>
          {knownGood !== undefined && (
            <p>
              {knownGood && sameMods(freeze.mods, knownGood.mods)
                ? `This is the setup last seen working (${new Date(knownGood.recorded_at).toLocaleString()}).`
                : "This setup has not been confirmed working yet. Play a session from the manager to confirm it."}
            </p>
          )}
          <p className="text-[var(--fg-muted)]">
            Nothing can be installed or removed until you unfreeze it.
            Unfreezing changes nothing else, and the restore point saved when
            freezing keeps this setup, with its settings, afterwards.
          </p>
          {drift &&
            (drifted ? (
              <div className="space-y-1">
                <p className="font-semibold">Changed since the freeze</p>
                <ul className="list-disc pl-4">
                  {drift.enabledChanged.map((d) => (
                    <li key={d.name}>
                      {d.name} is now {d.nowEnabled ? "enabled" : "disabled"}
                    </li>
                  ))}
                  {drift.added.map((name) => (
                    <li key={`+${name}`}>{name} was added</li>
                  ))}
                  {drift.removed.map((name) => (
                    <li key={`-${name}`}>{name} is missing</li>
                  ))}
                  {settingsChanged.map((id) => (
                    <li key={`s-${id}`}>{nameOf(id)}'s settings changed</li>
                  ))}
                </ul>
              </div>
            ) : (
              <p>Matches the frozen state.</p>
            ))}
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() => run(() => api.unfreezeProfile(profileId))}
          >
            Unfreeze
          </Button>
        </div>
      ) : (
        <form
          className="text-xs space-y-2"
          onSubmit={(event) => {
            event.preventDefault();
            void freezeNow();
          }}
        >
          <p className="text-[var(--fg-muted)]">
            Keeps the mods and versions exactly as they are now, for example so
            everyone in a multiplayer group stays in step.
          </p>
          <label className="block space-y-1">
            <span className="font-medium">Reason (optional)</span>
            <input
              type="text"
              maxLength={200}
              value={reason}
              onChange={(event) => setReason(event.target.value)}
              placeholder="e.g. Saturday co-op"
              className="w-full px-3 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)]"
            />
          </label>
          <Button size="sm" type="submit" disabled={busy}>
            Freeze
          </Button>
        </form>
      )}
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
