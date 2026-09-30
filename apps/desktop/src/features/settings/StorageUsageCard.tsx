import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { StorageUsageDto } from "@/shared/api/generated";
import { formatBytes } from "./StorageCleanupCard";
import { PieChart } from "lucide-react";

const show = (bytes: number | null) =>
  bytes === null ? "could not be read" : formatBytes(bytes);

/**
 * How much space the manager uses and where, measured from the files on disk
 * when asked. It only reads; removing anything is done by Storage cleanup.
 */
export const StorageUsageCard: React.FC = () => {
  const [usage, setUsage] = useState<StorageUsageDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const measure = async () => {
    setBusy(true);
    setError(null);
    try {
      setUsage(await api.getStorageUsage());
    } catch (measureError) {
      setError(errorSummary(measureError, "Storage could not be measured"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <PieChart className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Space used</h3>
      </div>
      <Button size="sm" variant="secondary" onClick={measure} isLoading={busy}>
        {usage ? "Measure again" : "Measure"}
      </Button>
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
      {usage && (
        <div className="text-xs space-y-3">
          <table className="w-full">
            <thead>
              <tr className="text-left text-[var(--fg-muted)]">
                <th className="py-1 font-medium">Profile</th>
                <th className="py-1 font-medium">Loaded mods</th>
                <th className="py-1 font-medium">Disabled mods</th>
                <th className="py-1 font-medium">Change leftovers</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[var(--border)]">
              {usage.profiles.map((profile) => (
                <tr key={profile.profile_id}>
                  <td className="py-1 pr-2">
                    {profile.name}
                    {profile.archived ? " (archived)" : ""}
                  </td>
                  <td className="py-1">{show(profile.live_bytes)}</td>
                  <td className="py-1">{show(profile.disabled_bytes)}</td>
                  <td className="py-1">{show(profile.operations_bytes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-1">
            <dt>Downloaded mod archives</dt>
            <dd>{show(usage.packages_bytes)}</dd>
            <dt>SMAPI installer files</dt>
            <dd>{show(usage.installer_cache_bytes)}</dd>
            <dt>Save backups</dt>
            <dd>{show(usage.save_backups_bytes)}</dd>
            <dt>Deleted profiles (trash)</dt>
            <dd>{show(usage.trash_bytes)}</dd>
          </dl>
          <p className="text-[var(--fg-muted)]">
            Archives are counted once however many profiles use them. To free
            space, use Storage cleanup below.
          </p>
        </div>
      )}
    </Card>
  );
};
