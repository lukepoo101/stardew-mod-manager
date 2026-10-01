import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useUnfinishedCopies } from "@/shared/api/hooks";
import { AlertTriangle } from "lucide-react";

/**
 * Duplicates that were interrupted before all their mods were installed,
 * for example because the app closed. Finishing installs only what is
 * missing, from what was recorded when copying started.
 */
export const UnfinishedCopies: React.FC = () => {
  const { data: copies, refetch } = useUnfinishedCopies();
  const [busy, setBusy] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!copies || copies.length === 0) {
    return status ? (
      <p role="status" className="text-xs">
        {status}
      </p>
    ) : null;
  }

  const finish = async (profileId: string) => {
    setBusy(profileId);
    setError(null);
    setStatus(null);
    try {
      const result = await api.finishProfileCopy(profileId);
      setStatus(
        `Finished "${result.profile_name}": ${result.installed.length} mod(s) installed now${
          result.failures.length > 0
            ? `, ${result.failures.length} could not be copied (${result.failures
                .map((f) => f.name)
                .join(", ")})`
            : ""
        }.`,
      );
      await refetch();
    } catch (finishError) {
      setError(errorSummary(finishError, "The copy was not finished"));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card className="space-y-2 border border-[var(--warning)]/40">
      <div className="flex items-center gap-2">
        <AlertTriangle className="w-4 h-4 text-[var(--warning)]" />
        <h3 className="font-bold text-sm">Unfinished copies</h3>
      </div>
      <p className="text-xs text-[var(--fg-muted)]">
        These duplicates were interrupted before all their mods were installed.
        Finishing installs only what is missing, using the mod list recorded
        when copying started. You can also delete the copy instead.
      </p>
      <ul className="text-xs space-y-1.5">
        {copies.map((copy) => (
          <li
            key={copy.profile_id}
            className="flex items-center justify-between gap-2"
          >
            <span>
              <span className="font-semibold">{copy.profile_name}</span>, a copy
              of {copy.source_name} ({copy.expected_mods} mod(s) expected)
            </span>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => finish(copy.profile_id)}
              disabled={busy !== null}
              isLoading={busy === copy.profile_id}
            >
              Finish copying
            </Button>
          </li>
        ))}
      </ul>
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
