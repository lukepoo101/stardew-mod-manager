import React, { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";

/**
 * Deleted profiles whose folders are still in the trash. Bringing one back
 * rebuilds it from the stored archives and copies its settings back.
 */
export const RecentlyDeleted: React.FC = () => {
  const { data: deleted, refetch } = useQuery({
    queryKey: ["deleted-profiles"],
    queryFn: () => api.listDeletedProfiles(),
  });
  const [busy, setBusy] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!deleted || (deleted.length === 0 && !status)) return null;

  const bringBack = async (entry: string, name: string) => {
    setBusy(entry);
    setError(null);
    setStatus(null);
    try {
      const result = await api.bringBackProfile(entry);
      setStatus(
        `Brought back "${result.profile_name}" with ${result.installed.length} mod(s)${
          result.failures.length > 0
            ? `; not brought back: ${result.failures.map((f) => f.name).join(", ")}`
            : ""
        }.`,
      );
      await refetch();
    } catch (bringError) {
      setError(errorSummary(bringError, `"${name}" was not brought back`));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-2">
      <h3 className="text-sm font-bold">Recently deleted</h3>
      <p className="text-xs text-[var(--fg-muted)]">
        These profiles' folders are still in the trash. Bringing one back
        installs its mods again from the stored archives, at the same versions
        and enabled state, and restores their settings. Storage cleanup removes
        trash after a while.
      </p>
      <ul className="text-xs space-y-1">
        {deleted.map((profile) => (
          <li
            key={profile.entry}
            className="flex items-center justify-between gap-2"
          >
            <span>
              <span className="font-semibold">{profile.name}</span>, deleted{" "}
              {new Date(profile.deleted_at).toLocaleString()} ·{" "}
              {profile.mod_count} mod(s)
            </span>
            <Button
              size="sm"
              variant="secondary"
              disabled={busy !== null}
              isLoading={busy === profile.entry}
              onClick={() => void bringBack(profile.entry, profile.name)}
            >
              Bring back
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
    </div>
  );
};
