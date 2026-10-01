import React, { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { RetentionPolicyDto } from "@/shared/api/generated";

const FIELDS: {
  key: keyof RetentionPolicyDto;
  label: string;
  min: number;
  max: number;
}[] = [
  {
    key: "keep_save_backups",
    label: "Backups kept for each save",
    min: 1,
    max: 100,
  },
  {
    key: "keep_settings_backups",
    label: "Backups kept for each mod's settings",
    min: 1,
    max: 100,
  },
  {
    key: "keep_trash_days",
    label: "Days a deleted profile stays in the trash",
    min: 1,
    max: 3650,
  },
];

/**
 * How much recovery data cleanup keeps. Changing it only changes what the
 * next check offers; nothing is removed until a cleanup is run.
 */
export const RetentionSettings: React.FC<{ onSaved?: () => void }> = ({
  onSaved,
}) => {
  const [policy, setPolicy] = useState<RetentionPolicyDto | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .getRetentionPolicy()
      .then(setPolicy)
      .catch((loadError) =>
        setError(errorSummary(loadError, "Could not read what is kept")),
      );
  }, []);

  if (!policy) {
    return error ? (
      <p role="alert" className="text-xs text-[var(--danger)]">
        {error}
      </p>
    ) : null;
  }

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setStatus(null);
    try {
      setPolicy(await api.setRetentionPolicy(policy));
      setStatus("Saved. The next check uses these limits.");
      onSaved?.();
    } catch (saveError) {
      setError(errorSummary(saveError, "The limits were not saved"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={save} className="space-y-2 text-xs">
      <fieldset className="space-y-1.5">
        <legend className="font-semibold mb-1">What cleanup keeps</legend>
        {FIELDS.map((field) => (
          <label key={field.key} className="flex items-center gap-2">
            <input
              type="number"
              min={field.min}
              max={field.max}
              value={policy[field.key]}
              onChange={(event) =>
                setPolicy({
                  ...policy,
                  [field.key]: Number(event.target.value),
                })
              }
              className="w-20 px-2 py-1 rounded border border-[var(--border)] bg-[var(--bg-primary)]"
            />
            <span>{field.label}</span>
          </label>
        ))}
      </fieldset>
      <p className="text-[var(--fg-muted)]">
        Older items become removable in the next check; nothing is removed until
        you run a cleanup. Restore points, the last working setup and anything a
        profile uses are always kept.
      </p>
      <Button size="sm" variant="secondary" type="submit" isLoading={busy}>
        Save limits
      </Button>
      {status && <p role="status">{status}</p>}
      {error && (
        <p role="alert" className="text-[var(--danger)]">
          {error}
        </p>
      )}
    </form>
  );
};
