import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useExperiments,
  useProfiles,
  useSaves,
} from "@/shared/api/hooks";
import { FlaskConical } from "lucide-react";

/**
 * Try changes on a copy of the active profile. The original is never changed
 * by the experiment; keeping it makes it an ordinary profile, discarding it
 * switches back to the original and deletes the copy.
 */
export const ExperimentCard: React.FC = () => {
  const { data: overview } = useActiveProfileOverview();
  const { data: experiments } = useExperiments();
  const { data: profiles } = useProfiles();
  const { data: saves } = useSaves();
  const [backupSaveId, setBackupSaveId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const active = overview?.profile;
  if (!active) return null;
  const current = experiments?.find((e) => e.profile_id === active.id);
  const sourceExists = Boolean(
    current && profiles?.some((p) => p.id === current.source_profile_id),
  );

  const run = async (action: () => Promise<string>) => {
    setBusy(true);
    setError(null);
    setStatus(null);
    try {
      setStatus(await action());
    } catch (actionError) {
      setError(errorSummary(actionError, "That did not work"));
    } finally {
      setBusy(false);
    }
  };

  const saveName = (id: string) => {
    const save = saves?.saves.find((candidate) => candidate.id === id);
    return save?.farm_name ? `${save.farm_name} (${id})` : id;
  };

  const start = () =>
    run(async () => {
      // A requested backup must exist before anything changes; if it cannot
      // be made, the experiment does not start.
      let backedUp = "";
      if (backupSaveId) {
        const backup = await api.backupSave(
          backupSaveId,
          `Made before the experiment "${active.name} experiment"`,
        );
        backedUp = ` ${saveName(backupSaveId)} was backed up at ${new Date(
          backup.created_at,
        ).toLocaleString()}.`;
      }
      const copy = await api.startExperiment(
        active.id,
        `${active.name} experiment`,
      );
      await api.activateProfile(copy.profile_id);
      return `Now using "${copy.profile_name}". "${active.name}" is unchanged.${backedUp}`;
    });

  const keep = () =>
    run(async () => {
      if (current) await api.keepExperiment(current.profile_id);
      return `"${active.name}" is now an ordinary profile.`;
    });

  const discard = () => {
    if (!current) return;
    if (
      !window.confirm(
        `Discard "${active.name}"? You will switch back to "${current.source_name}" and the experiment will be deleted.`,
      )
    )
      return;
    void run(async () => {
      await api.activateProfile(current.source_profile_id);
      await api.archiveProfile(current.profile_id);
      await api.deleteProfile(current.profile_id);
      await api.keepExperiment(current.profile_id);
      return `Back on "${current.source_name}". The experiment was deleted.`;
    });
  };

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <FlaskConical className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">
          {current ? "You are in an experiment" : "Try changes safely"}
        </h3>
      </div>
      {current ? (
        <div className="text-xs space-y-2">
          <p>
            "{active.name}" is a copy of "{current.source_name}" made{" "}
            {new Date(current.created_at).toLocaleString()}. Changes here never
            reach "{current.source_name}".
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={keep}
            >
              Keep as a normal profile
            </Button>
            <Button
              size="sm"
              variant="danger"
              disabled={busy || !sourceExists}
              onClick={discard}
              title={
                sourceExists
                  ? undefined
                  : "The original profile no longer exists"
              }
            >
              Discard and go back to "{current.source_name}"
            </Button>
          </div>
        </div>
      ) : (
        <div className="text-xs space-y-2">
          <p className="text-[var(--fg-muted)]">
            Makes a copy of "{active.name}" with its {active.mod_count} mod(s)
            and switches to it, so you can install, remove or disable mods
            without risk. "{active.name}" stays exactly as it is.
          </p>
          {saves && saves.saves.length > 0 && (
            <label className="flex flex-wrap items-center gap-2">
              <span>Back up a save first:</span>
              <select
                value={backupSaveId}
                onChange={(event) => setBackupSaveId(event.target.value)}
                className="px-2 py-1 rounded border border-[var(--border)] bg-[var(--bg-primary)]"
              >
                <option value="">No save backup</option>
                {saves.saves.map((save) => (
                  <option key={save.id} value={save.id}>
                    {saveName(save.id)}
                  </option>
                ))}
              </select>
            </label>
          )}
          {backupSaveId && (
            <p className="text-[var(--fg-muted)]">
              The save is copied to the manager's backups and checked before the
              experiment starts. If that fails, nothing changes. Mod changes
              never touch saves; restore the backup from Saves if you need it.
            </p>
          )}
          <Button size="sm" disabled={busy} isLoading={busy} onClick={start}>
            Start an experiment
          </Button>
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
