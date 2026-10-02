import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useKnownGood,
  useProfileMods,
  useRecentOperations,
} from "@/shared/api/hooks";
import {
  healthChanges,
  knownGoodDiff,
  restorePlan,
} from "@/shared/profiles/knownGood";
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
  const { data: operations } = useRecentOperations(100);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!profileId || !mods) return null;
  const since =
    record && operations
      ? operations.filter(
          (op) =>
            op.profile_id === profileId &&
            op.state === "succeeded" &&
            Date.parse(op.created_at) > Date.parse(record.recorded_at),
        )
      : [];
  const diff = record ? knownGoodDiff(record.mods, mods) : null;
  const plan = diff ? restorePlan(diff) : null;
  const health =
    record?.findings && overview
      ? healthChanges(record.findings, overview.health_summary.findings)
      : null;
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
          <HealthSince health={health} hasBaseline={Boolean(record.findings)} />
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
              {since.length > 0 && (
                <div>
                  <p className="font-semibold">Changes made since</p>
                  <ul className="list-disc pl-4">
                    {since.map((op) => (
                      <li key={op.id}>
                        {op.kind.replaceAll("_", " ")},{" "}
                        {new Date(op.created_at).toLocaleString()}
                      </li>
                    ))}
                  </ul>
                  <p className="text-[var(--fg-muted)]">
                    These happened after the last good session, so they are
                    worth checking first. That is timing, not proof that one of
                    them caused a problem. Activity shows what each one changed.
                  </p>
                </div>
              )}
            </>
          )}
        </div>
      )}
      {record && (
        <button
          type="button"
          className="text-xs underline text-[var(--fg-muted)] cursor-pointer"
          disabled={busy}
          onClick={async () => {
            const answer = window.prompt(
              'Forgetting the last working setup removes it as a way back: you can no longer restore it, and its packages stay kept only while a restore point lists them. A newer one is recorded the next time this profile works. Type "forget" to confirm.',
            );
            if (answer?.trim().toLowerCase() !== "forget") return;
            setError(null);
            try {
              await api.forgetKnownGood(profileId);
              setStatus("The last working setup was forgotten.");
            } catch (forgetError) {
              setError(errorSummary(forgetError, "It was not forgotten"));
            }
          }}
        >
          Forget this record...
        </button>
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

/** Health findings compared with when the profile last worked. */
const HealthSince: React.FC<{
  health: ReturnType<typeof healthChanges> | null;
  hasBaseline: boolean;
}> = ({ health, hasBaseline }) => {
  if (!hasBaseline || !health)
    return (
      <p className="text-[var(--fg-muted)]">
        Health then was not recorded, so it cannot be compared yet. It will be
        after the next session where SMAPI loads this profile's mods.
      </p>
    );
  const { introduced, resolved, escalated, unchanged } = health;
  if (introduced.length + resolved.length + escalated.length === 0)
    return (
      <p>
        Health is the same as then
        {unchanged > 0 ? ` (${unchanged} finding(s) were already there)` : ""}.
      </p>
    );
  return (
    <div>
      <p className="font-semibold">Health since then</p>
      <ul className="list-disc pl-4 space-y-0.5">
        {introduced.map((f) => (
          <li key={`n:${f.fingerprint}`}>
            New: {f.title} ({f.severity})
          </li>
        ))}
        {escalated.map(({ finding, was }) => (
          <li key={`x:${finding.fingerprint}`}>
            Worse: {finding.title} (was {was}, now {finding.severity})
          </li>
        ))}
        {resolved.map((f) => (
          <li key={`r:${f.fingerprint}`}>Gone: {f.title}</li>
        ))}
      </ul>
      {unchanged > 0 && (
        <p className="text-[var(--fg-muted)]">
          {unchanged} other finding(s) were already there when it worked.
        </p>
      )}
      <p className="text-[var(--fg-muted)]">
        New findings appeared after the last good session. That is timing, not
        proof that they explain a problem.
      </p>
    </div>
  );
};
