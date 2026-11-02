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

type Draft = Record<keyof RetentionPolicyDto, string>;

const toDraft = (policy: RetentionPolicyDto): Draft => ({
  keep_save_backups: String(policy.keep_save_backups),
  keep_settings_backups: String(policy.keep_settings_backups),
  keep_trash_days: String(policy.keep_trash_days),
});

/**
 * How much recovery data cleanup keeps. Changing it only changes what the
 * next check offers; nothing is removed until a cleanup is run.
 */
export const RetentionSettings: React.FC<{ onSaved?: () => void }> = ({
  onSaved,
}) => {
  // Kept as typed, so an empty or invalid field is reported, not read as 0.
  const [draft, setDraft] = useState<Draft | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .getRetentionPolicy()
      .then((policy) => setDraft(toDraft(policy)))
      .catch((loadError) =>
        setError(errorSummary(loadError, "Could not read what is kept")),
      );
  }, []);

  if (!draft) {
    return error ? (
      <p role="alert" className="text-xs text-[var(--danger)]">
        {error}
      </p>
    ) : null;
  }

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    setStatus(null);
    setError(null);
    const invalid = FIELDS.filter((field) => {
      const text = draft[field.key].trim();
      const value = Number(text);
      return (
        !/^\d+$/.test(text) ||
        !Number.isInteger(value) ||
        value < field.min ||
        value > field.max
      );
    });
    if (invalid.length > 0) {
      setError(
        `Nothing was saved. Enter a whole number: ${invalid
          .map((field) => `${field.label} from ${field.min} to ${field.max}`)
          .join("; ")}.`,
      );
      return;
    }
    setBusy(true);
    try {
      const saved = await api.setRetentionPolicy({
        keep_save_backups: Number(draft.keep_save_backups),
        keep_settings_backups: Number(draft.keep_settings_backups),
        keep_trash_days: Number(draft.keep_trash_days),
      });
      setDraft(toDraft(saved));
      setStatus("Saved. The next check uses these limits.");
      onSaved?.();
    } catch (saveError) {
      setError(errorSummary(saveError, "The limits were not saved"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={save} noValidate className="space-y-2 text-xs">
      <fieldset className="space-y-1.5">
        <legend className="font-semibold mb-1">What cleanup keeps</legend>
        {FIELDS.map((field) => (
          <label key={field.key} className="flex items-center gap-2">
            <input
              type="number"
              inputMode="numeric"
              min={field.min}
              max={field.max}
              step={1}
              value={draft[field.key]}
              aria-invalid={error !== null}
              onChange={(event) => {
                setStatus(null);
                setDraft({ ...draft, [field.key]: event.target.value });
              }}
              className="w-20 px-2 py-1 rounded border border-[var(--border)] bg-[var(--bg-primary)]"
            />
            <span>
              {field.label}{" "}
              <span className="text-[var(--fg-muted)]">
                ({field.min} to {field.max})
              </span>
            </span>
          </label>
        ))}
      </fieldset>
      <p className="text-[var(--fg-muted)]">
        Older items become removable in the next check; nothing is removed until
        you run a cleanup. Restore points, the last working setup, deleted
        profiles that can still be brought back, and anything a profile uses are
        always kept.
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
