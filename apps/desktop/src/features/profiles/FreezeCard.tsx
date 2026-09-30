import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useProfileFreeze,
  useProfileMods,
} from "@/shared/api/hooks";
import { freezeDrift } from "@/shared/profiles/freezeDrift";
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

  if (!profileId) return null;

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
  const drifted =
    drift &&
    drift.enabledChanged.length + drift.added.length + drift.removed.length > 0;

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
          </p>
          <p className="text-[var(--fg-muted)]">
            Nothing can be installed or removed until you unfreeze it.
            Unfreezing changes nothing else.
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
            void run(() => api.freezeProfile(profileId, reason));
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
