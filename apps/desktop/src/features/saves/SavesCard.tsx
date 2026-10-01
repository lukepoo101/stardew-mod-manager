import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { StatusBadge } from "@/components/ui/StatusBadge";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import {
  useActiveProfileOverview,
  useProfiles,
  useSaves,
} from "@/shared/api/hooks";
import type { SaveDto } from "@/shared/api/generated";
import { Sprout } from "lucide-react";

const label = (save: SaveDto) =>
  save.farm_name ? `${save.farm_name} Farm` : save.id;

/**
 * Stardew Valley saves: which profile each one usually goes with, and backups
 * the manager can make and restore. Nothing here changes mods, and nothing
 * changes a save except an explicit restore.
 */
export const SavesCard: React.FC = () => {
  const { data } = useSaves();
  const { data: profiles } = useProfiles();
  const { data: overview } = useActiveProfileOverview();
  const [busy, setBusy] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async (key: string, action: () => Promise<string>) => {
    setBusy(key);
    setError(null);
    setStatus(null);
    try {
      setStatus(await action());
    } catch (actionError) {
      setError(errorSummary(actionError, "That did not work"));
    } finally {
      setBusy(null);
    }
  };

  const latest = data?.saves[0];
  const mismatch =
    latest?.profile_id &&
    overview &&
    latest.profile_id !== overview.profile.id &&
    latest.profile_name;

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Sprout className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Saves</h3>
      </div>
      {!data ? null : !data.saves_dir ? (
        <p className="text-xs text-[var(--fg-muted)]">
          The Stardew Valley save folder could not be located on this computer.
        </p>
      ) : data.saves.length === 0 ? (
        <p className="text-xs text-[var(--fg-muted)]">
          No saves yet. Start a farm in the game and it will appear here.
        </p>
      ) : (
        <div className="space-y-3 text-xs">
          <p className="text-[var(--fg-muted)]">
            Link each farm to the profile you play it with. Links and backups
            never change your mods, and the manager only changes a save when you
            restore a backup.
          </p>
          {mismatch && (
            <p role="alert" className="text-[var(--warning)]">
              Your most recently played farm, {label(latest)}, is linked to "
              {latest.profile_name}", but "{overview.profile.name}" is active.
              Loading it with different mods may change or break things in that
              save. This is based on which save was played last, not on which
              one you will load.
            </p>
          )}
          <ul className="divide-y divide-[var(--border)] border border-[var(--border)] rounded-lg">
            {data.saves.map((save) => (
              <li key={save.id} className="p-3 space-y-2">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <p className="font-semibold">{label(save)}</p>
                    <p className="text-[var(--fg-muted)]">
                      {save.farmer_name ?? "Unknown farmer"}
                      {save.game_version ? ` · game ${save.game_version}` : ""}
                      {save.modified_at
                        ? ` · played ${new Date(save.modified_at).toLocaleString()}`
                        : ""}
                    </p>
                  </div>
                  <label className="flex items-center gap-2">
                    <span>Usual profile</span>
                    <select
                      value={save.profile_id ?? ""}
                      disabled={busy !== null}
                      onChange={(event) => {
                        const value = event.target.value || null;
                        void run(`link:${save.id}`, async () => {
                          await api.associateSave(save.id, value);
                          return value
                            ? `${label(save)} is linked to a profile.`
                            : `${label(save)} is no longer linked.`;
                        });
                      }}
                      className="px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)]"
                    >
                      <option value="">None</option>
                      {profiles?.map((profile) => (
                        <option key={profile.id} value={profile.id}>
                          {profile.name}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
                {save.profile_id && !save.profile_name && (
                  <StatusBadge variant="warning">
                    Linked profile no longer exists
                  </StatusBadge>
                )}
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={busy !== null}
                    isLoading={busy === `backup:${save.id}`}
                    onClick={() =>
                      run(`backup:${save.id}`, async () => {
                        await api.backupSave(save.id);
                        return `Backed up ${label(save)} and checked the copy.`;
                      })
                    }
                  >
                    Back up now
                  </Button>
                  <span className="text-[var(--fg-muted)]">
                    {save.backups.length} backup(s)
                  </span>
                </div>
                {save.backups.length > 0 && (
                  <details>
                    <summary className="cursor-pointer">Backups</summary>
                    <ul className="mt-1 space-y-1">
                      {save.backups.map((backup) => (
                        <li
                          key={backup.id}
                          className="flex items-center justify-between gap-2"
                        >
                          <span>
                            {new Date(backup.created_at).toLocaleString()}
                            {backup.id.endsWith("-before-restore")
                              ? " (kept before a restore)"
                              : ""}
                          </span>
                          <Button
                            size="sm"
                            variant="ghost"
                            disabled={busy !== null}
                            onClick={() => {
                              if (
                                !window.confirm(
                                  `Restore ${label(save)} to ${new Date(backup.created_at).toLocaleString()}? The current save is backed up first. Your mods are not changed.`,
                                )
                              )
                                return;
                              void run(`restore:${backup.id}`, async () => {
                                await api.restoreSaveBackup(backup.id);
                                return `Restored ${label(save)}. The save it replaced was kept as a backup.`;
                              });
                            }}
                          >
                            Restore...
                          </Button>
                        </li>
                      ))}
                    </ul>
                  </details>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
      {data && data.unavailable_links.length > 0 && (
        <div className="text-xs space-y-1">
          <p className="font-semibold">Linked saves that are not found</p>
          <p className="text-[var(--fg-muted)]">
            These were linked to a profile but are no longer in the save folder
            (moved, renamed or deleted). The links are kept until you remove
            them.
          </p>
          <ul className="space-y-1">
            {data.unavailable_links.map((link) => (
              <li
                key={link.save_id}
                className="flex items-center justify-between gap-2"
              >
                <span>
                  {link.save_id}, linked to{" "}
                  {link.profile_name ?? "a profile that no longer exists"}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy !== null}
                  onClick={() =>
                    run(`unlink:${link.save_id}`, async () => {
                      await api.associateSave(link.save_id, null);
                      return `Removed the link for ${link.save_id}.`;
                    })
                  }
                >
                  Remove link
                </Button>
              </li>
            ))}
          </ul>
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
