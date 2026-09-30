import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { ConfigBackupDto } from "@/shared/api/generated";

/**
 * Copies of a mod's settings saved before it was replaced or reinstalled.
 * Restoring writes them back into the mod's folder; nothing else changes.
 */
export const SettingsBackups: React.FC<{ profileComponentId: string }> = ({
  profileComponentId,
}) => {
  const [backups, setBackups] = useState<ConfigBackupDto[] | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const load = async () => {
    if (backups) return;
    try {
      setBackups(await api.listConfigBackups(profileComponentId));
    } catch (error) {
      setStatus(errorSummary(error, "The backups could not be listed"));
    }
  };

  const restore = async (backup: ConfigBackupDto) => {
    if (
      !window.confirm(
        `Put back the settings saved ${new Date(backup.created_at).toLocaleString()}? The mod's current ${backup.files.join(", ")} will be replaced. Close the game first.`,
      )
    )
      return;
    try {
      await api.restoreConfigBackup(profileComponentId, backup.id);
      setStatus("Settings restored.");
    } catch (error) {
      setStatus(errorSummary(error, "The settings were not restored"));
    }
  };

  return (
    <details
      className="text-xs"
      onToggle={(event) => {
        if ((event.target as HTMLDetailsElement).open) void load();
      }}
    >
      <summary className="cursor-pointer font-bold text-[var(--fg-muted)] uppercase tracking-wider">
        Settings backups
      </summary>
      {backups && backups.length === 0 && (
        <p className="mt-1 text-[var(--fg-muted)]">
          None yet. Settings are backed up automatically before this mod is
          replaced or reinstalled.
        </p>
      )}
      {backups && backups.length > 0 && (
        <ul className="mt-1 space-y-1">
          {backups.map((backup) => (
            <li
              key={backup.id}
              className="flex items-center justify-between gap-2"
            >
              <span>
                {new Date(backup.created_at).toLocaleString()}:{" "}
                {backup.files.join(", ")}
              </span>
              <Button size="sm" variant="ghost" onClick={() => restore(backup)}>
                Restore
              </Button>
            </li>
          ))}
        </ul>
      )}
      {status && (
        <p role="status" className="mt-1">
          {status}
        </p>
      )}
    </details>
  );
};
