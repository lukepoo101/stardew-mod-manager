import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useKnownGood,
  useProfileMods,
} from "@/shared/api/hooks";
import { knownGoodDiff, restorePlan } from "@/shared/profiles/knownGood";
import { ShieldCheck } from "lucide-react";

/**
 * What changed since the active profile last worked, meaning since SMAPI was
 * last confirmed to have loaded its mods. Restoring only switches mods on and
 * off; anything else it cannot undo is listed.
 */
export const KnownGoodCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const { data: record } = useKnownGood(profileId);
  const { data: mods } = useProfileMods(profileId);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!profileId || !mods) return null;
  const diff = record ? knownGoodDiff(record.mods, mods) : null;
  const plan = diff ? restorePlan(diff) : null;
  const changed =
    diff &&
    diff.enabledChanged.length +
      diff.added.length +
      diff.removed.length +
      diff.versionChanged.length >
      0;

  const restore = async () => {
    if (!plan) return;
    const count = plan.enable.length + plan.disable.length;
    if (
      !window.confirm(
        `Switch ${count} mod(s) back to how they were when this profile last worked? Nothing is installed or removed.`,
      )
    )
      return;
    setBusy(true);
    setError(null);
    setStatus(null);
    try {
      const failed: string[] = [];
      if (plan.enable.length > 0) {
        const result = await api.setModsEnabled(plan.enable, true);
        failed.push(...result.failed.map((f) => `${f.name}: ${f.message}`));
      }
      if (plan.disable.length > 0) {
        const result = await api.setModsEnabled(plan.disable, false);
        failed.push(...result.failed.map((f) => `${f.name}: ${f.message}`));
      }
      setStatus(
        failed.length === 0
          ? "Enabled state restored."
          : `Some mods could not be switched: ${failed.join("; ")}`,
      );
    } catch (restoreError) {
      setError(errorSummary(restoreError, "Nothing was restored"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <ShieldCheck className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Last known good</h3>
      </div>
      {!record || !diff ? (
        <p className="text-xs text-[var(--fg-muted)]">
          Recorded the first time SMAPI is confirmed to have loaded this
          profile's mods. Starting the game alone does not count.
        </p>
      ) : (
        <div className="text-xs space-y-2">
          <p>
            Worked {new Date(record.recorded_at).toLocaleString()} with{" "}
            {record.mods.length} mod(s)
            {record.game_version
              ? `, Stardew Valley ${record.game_version}`
              : ""}
            {record.smapi_version ? `, SMAPI ${record.smapi_version}` : ""}.
          </p>
          {!changed ? (
            <p>Nothing has changed since.</p>
          ) : (
            <>
              <p className="font-semibold">Changed since</p>
              <ul className="list-disc pl-4 space-y-0.5">
                {diff.enabledChanged.map((c) => (
                  <li key={`e:${c.mod.profile_component_id}`}>
                    {c.mod.name} was {c.wasEnabled ? "enabled" : "disabled"}
                  </li>
                ))}
                {diff.added.map((m) => (
                  <li key={`a:${m.profile_component_id}`}>
                    {m.name} was installed since
                  </li>
                ))}
                {diff.versionChanged.map((c) => (
                  <li key={`v:${c.mod.profile_component_id}`}>
                    {c.mod.name} is {c.mod.version}, was {c.was} (install{" "}
                    {c.was} again to go back)
                  </li>
                ))}
                {diff.removed.map((m) => (
                  <li key={`r:${m.unique_id}`}>
                    {m.name} {m.version} was removed (install it again to go
                    back)
                  </li>
                ))}
              </ul>
              {plan && plan.enable.length + plan.disable.length > 0 && (
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={busy}
                  isLoading={busy}
                  onClick={restore}
                >
                  Restore enabled state
                </Button>
              )}
              <p className="text-[var(--fg-muted)]">
                Restoring switches mods on and off, and turns off mods installed
                since. It does not change versions or reinstall removed mods.
              </p>
            </>
          )}
        </div>
      )}
      {status && (
        <p role="status" className="text-xs">
          {status}
        </p>
      )}
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
