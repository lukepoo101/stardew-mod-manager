import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { ConfigBackupDto } from "@/shared/api/generated";
import { askToConfirm } from "@/shared/platform/confirm";

/**
 * Copies of a mod's settings saved before it was replaced or reinstalled.
 * Restoring writes them back into the mod's folder; nothing else changes, and
 * the settings being replaced are backed up first. Backups belong to the
 * profile and the mod's UniqueID, not to one installed copy, so they stay
 * reachable after a reinstall or a version change.
 */
export const SettingsBackups: React.FC<{
  profileId: string;
  uniqueId: string;
}> = ({ profileId, uniqueId }) => {
  const [backups, setBackups] = useState<ConfigBackupDto[] | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  // Read again each time the list is opened: a reinstall or restore since
  // the last look adds backups.
  const load = async () => {
    try {
      setBackups(await api.listConfigBackups(profileId, uniqueId));
    } catch (error) {
      setStatus(errorSummary(error, "The backups could not be listed"));
    }
  };

  const restore = async (backup: ConfigBackupDto) => {
    if (
      !(await askToConfirm(
        `Put back the settings saved ${new Date(backup.created_at).toLocaleString()}? The mod's current ${backup.files.join(", ")} will be replaced, after a backup of them is saved. Close the game first.`,
      ))
    )
      return;
    try {
      const result = await api.restoreConfigBackup(
        profileId,
        uniqueId,
        backup.id,
      );
      setStatus(
        result.saved_current
          ? "Settings restored. The settings they replaced were backed up first and are listed here."
          : "Settings restored. Nothing else was saved: the mod's settings already matched this backup, or it had none.",
      );
      await load();
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
          replaced or reinstalled, and before a backup is restored.
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
