import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useKnownGood,
  useRestorePoints,
  useProfileMods,
  useRecentOperations,
} from "@/shared/api/hooks";
import {
  healthChanges,
  knownGoodDiff,
  restorePlan,
} from "@/shared/profiles/knownGood";
import { ShieldCheck } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useQuickFileCheck } from "@/shared/api/hooks";
import type {
  ModListItemDto,
  SettingFileHashDto,
} from "@/shared/api/generated";
import { settingsDrift } from "@/shared/profiles/freezeDrift";

/**
 * Settings and files changed since the record: settings by checksum (when
 * the record kept them), and mod files changed outside the manager.
 */
const SettingsAndFilesSince: React.FC<{
  profileId: string;
  recorded: SettingFileHashDto[] | null | undefined;
  mods: readonly ModListItemDto[];
}> = ({ profileId, recorded, mods }) => {
  const { data: hashes } = useQuery({
    queryKey: ["settings-hashes", profileId],
    queryFn: () => api.settingsHashes(profileId),
    enabled: Boolean(recorded),
  });
  const { data: files } = useQuickFileCheck(profileId);
  const nameOf = (id: string) =>
    mods.find((m) => m.unique_id.toLowerCase() === id)?.name ?? id;
  const settings = recorded && hashes ? settingsDrift(recorded, hashes) : null;
  const outside = (files ?? [])
    .filter((c) => c.status === "changed")
    .flatMap((c) => c.mods);
  return (
    <div className="space-y-0.5">
      <p>
        {!recorded
          ? "Settings were not recorded with this, so settings changes cannot be listed."
          : settings === null
            ? null
            : settings.length === 0
              ? "No mod settings changed since."
              : `Settings changed since: ${settings.map(nameOf).join(", ")}.`}
      </p>
      {outside.length > 0 && (
        <p>
          Files changed outside the manager (by size): {outside.join(", ")}.
          Check mod files on the Diagnostics page to review them.
        </p>
      )}
    </div>
  );
};

/**
 * What changed since the active profile last worked, meaning since SMAPI was
 * last confirmed to have loaded its mods. Restoring only switches mods on and
 * off; anything else it cannot undo is listed.
 */
export const KnownGoodCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const { data: knownGood } = useKnownGood(profileId);
  const { data: points } = useRestorePoints(profileId);
  // The baseline: the last working setup, or a restore point the user picks.
  const [baselineId, setBaselineId] = useState("");
  const point = points?.find((p) => p.id === baselineId);
  const record = point
    ? {
        profile_id: profileId ?? "",
        recorded_at: point.created_at,
        game_version: null,
        smapi_version: null,
        mods: point.mods,
        findings: null,
        settings: null,
      }
    : knownGood;
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
      {points && points.length > 0 && (
        <label className="flex items-center gap-2 text-xs">
          <span className="text-[var(--fg-muted)]">Compare with</span>
          <select
            value={baselineId}
            onChange={(event) => setBaselineId(event.target.value)}
            className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
          >
            <option value="">The last working setup</option>
            {points.map((p) => (
              <option key={p.id} value={p.id}>
                Restore point: {p.label}
              </option>
            ))}
          </select>
        </label>
      )}
      {!record || !diff ? (
        <p className="text-xs text-[var(--fg-muted)]">
          Recorded the first time SMAPI is confirmed to have loaded this
          profile's mods. Starting the game alone does not count.
        </p>
      ) : (
        <div className="text-xs space-y-2">
          <p>
            {point ? `Restore point saved` : "Worked"}{" "}
            {new Date(record.recorded_at).toLocaleString()} with{" "}
            {record.mods.length} mod(s)
            {record.game_version
              ? `, Stardew Valley ${record.game_version}`
              : ""}
            {record.smapi_version ? `, SMAPI ${record.smapi_version}` : ""}.
          </p>
          <HealthSince health={health} hasBaseline={Boolean(record.findings)} />
          <SettingsAndFilesSince
            profileId={profileId}
            recorded={"settings" in record ? record.settings : null}
            mods={mods}
          />
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
      {knownGood && !point && (
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
