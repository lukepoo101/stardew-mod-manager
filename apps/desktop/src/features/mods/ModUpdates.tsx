import React, { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { useModUpdates } from "@/shared/api/hooks";
import { errorSummary } from "@/shared/api/errors";
import { formatBytes } from "@/features/settings/StorageCleanupCard";
import type {
  DownloadedPreviewDto,
  ModUpdateDto,
  ModUpdatesDto,
} from "@/shared/api/generated";

/** Compatibility-list statuses worth pointing out. */
const NOTEWORTHY = new Set(["Broken", "Obsolete", "Abandoned", "Unofficial"]);

const checkedText = (at: string | null) =>
  at ? `Last checked ${new Date(at).toLocaleString()}.` : "Not checked yet.";

/** An update the manager can download, or try to: GitHub was not read. */
const downloadable = (mod: ModUpdateDto) =>
  mod.download !== null || mod.github_unread;

/** Where the file comes from, in one line. */
function sourceLine(mod: ModUpdateDto): string {
  const d = mod.download;
  if (!d) return "";
  return [
    `${d.asset_name} (${formatBytes(d.size_bytes)}) from GitHub release ${d.tag} of ${d.repo}`,
    d.declared_by_mod
      ? "named in the mod's manifest"
      : "named by the community compatibility list",
    d.has_checksum
      ? "checked against GitHub's checksum"
      : "GitHub publishes no checksum for it",
  ].join(" · ");
}

/** Why there is no download, and where to get the update instead. */
function noDownloadLine(mod: ModUpdateDto): string {
  const elsewhere = mod.update_url
    ? "Download it from the mod page, then install the archive here."
    : "Download it from wherever the mod is published, then install the archive here.";
  return mod.download_note ? `${mod.download_note} ${elsewhere}` : elsewhere;
}

/**
 * Checks the profile's mods for updates with smapi.io, the check SMAPI runs
 * when the game starts, and downloads an update from GitHub when the mod
 * publishes one there. A download goes to the same review as a chosen
 * archive; nothing is installed from here.
 */
export const ModUpdates: React.FC<{
  profileId: string;
  /** Busy elsewhere (a review is open or committing). */
  disabled?: boolean;
  onDownloaded: (downloaded: DownloadedPreviewDto) => void;
}> = ({ profileId, disabled = false, onDownloaded }) => {
  const { data } = useModUpdates(profileId);
  const queryClient = useQueryClient();
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<unknown>(null);
  const [preparing, setPreparing] = useState<string | null>(null);
  const [rowError, setRowError] = useState<{ id: string; error: unknown }>();

  const check = async () => {
    setChecking(true);
    setCheckError(null);
    setRowError(undefined);
    try {
      const fresh = await api.checkModUpdates(profileId);
      queryClient.setQueryData<ModUpdatesDto>(
        ["mod-updates", profileId],
        fresh,
      );
    } catch (error) {
      setCheckError(error);
    } finally {
      setChecking(false);
    }
  };

  const download = async (mod: ModUpdateDto) => {
    setPreparing(mod.profile_component_id);
    setRowError(undefined);
    try {
      onDownloaded(
        await api.prepareModUpdate(profileId, mod.profile_component_id),
      );
    } catch (error) {
      setRowError({ id: mod.profile_component_id, error });
    } finally {
      setPreparing(null);
    }
  };

  const openPage = async (mod: ModUpdateDto) => {
    if (!mod.update_url) return;
    setRowError(undefined);
    try {
      await api.openExternalPage(mod.update_url);
    } catch (error) {
      setRowError({ id: mod.profile_component_id, error });
    }
  };

  const mods = data?.mods ?? [];
  // Updates that can be downloaded here first, then the rest by name (the
  // backend already sorts by name).
  const updates = mods
    .filter((m) => m.update_version)
    .sort((a, b) => Number(downloadable(b)) - Number(downloadable(a)));
  const notes = mods.filter(
    (m) =>
      !m.update_version &&
      m.compatibility_status &&
      NOTEWORTHY.has(m.compatibility_status),
  );
  const unknown = mods.filter((m) => !m.update_version && m.errors.length > 0);
  const upToDate = mods.length - updates.length - unknown.length;
  const failure = checkError ? errorSummary(checkError) : data?.error;

  return (
    <section
      aria-labelledby="mod-updates-title"
      className="p-4 rounded-xl border border-[var(--border)] bg-[var(--bg-surface)] space-y-3 text-sm"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <h3 id="mod-updates-title" className="font-semibold">
            Updates
          </h3>
          <p className="text-xs text-[var(--fg-muted)]">
            {checkedText(data?.checked_at ?? null)} Checking asks smapi.io, as
            SMAPI does when the game starts, sending each mod's UniqueID,
            version and update keys with your SMAPI and game versions. For mods
            with an update it then reads their public GitHub release lists.
          </p>
        </div>
        <Button
          size="sm"
          variant="secondary"
          isLoading={checking}
          disabled={disabled || preparing !== null}
          onClick={() => void check()}
        >
          Check for updates
        </Button>
      </div>

      {failure && (
        <p
          role="alert"
          className="text-xs text-[var(--danger)] p-2 rounded-lg bg-[var(--danger-surface)]"
        >
          {failure}
          {data?.checked_at && mods.length > 0
            ? " The results below are from the last check that worked."
            : ""}
        </p>
      )}

      {data?.github_token_rejected && (
        <p
          role="alert"
          className="text-xs text-[var(--warning)] p-2 rounded-lg bg-[var(--warning-surface)]"
        >
          GitHub rejected your token, so releases were read without it and fewer
          repositories were checked. Replace it in Settings.
        </p>
      )}

      {data?.checked_at && !failure && mods.length === 0 && (
        <p role="status" className="text-xs">
          This profile has no mods to check.
        </p>
      )}

      {data?.checked_at && !failure && mods.length > 0 && (
        <p role="status" className="text-xs">
          {updates.length === 0
            ? "No updates found."
            : `${updates.length} update${updates.length === 1 ? "" : "s"} found.`}{" "}
          {upToDate} mod{upToDate === 1 ? " is" : "s are"} up to date
          {unknown.length > 0 ? `; ${unknown.length} could not be checked` : ""}
          .
        </p>
      )}

      {updates.length > 0 && (
        <ul className="space-y-2" aria-label="Mods with updates">
          {updates.map((mod) => (
            <li
              key={mod.profile_component_id}
              className="p-3 rounded-lg border border-[var(--border)] space-y-1"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="min-w-0 break-words">
                  <span className="font-medium">{mod.name}</span>{" "}
                  <span aria-hidden="true">
                    {mod.installed_version} → {mod.update_version}
                  </span>
                  <span className="sr-only">
                    from {mod.installed_version} to {mod.update_version}
                  </span>
                </span>
                <span className="flex gap-2">
                  {downloadable(mod) && (
                    <Button
                      size="sm"
                      variant="primary"
                      aria-label={`Download ${mod.name} ${mod.update_version}`}
                      isLoading={preparing === mod.profile_component_id}
                      disabled={disabled || checking || preparing !== null}
                      onClick={() => void download(mod)}
                    >
                      Download update
                    </Button>
                  )}
                  {mod.update_url && (
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Open ${mod.name}'s mod page`}
                      onClick={() => void openPage(mod)}
                    >
                      Open mod page
                    </Button>
                  )}
                </span>
              </div>
              <p className="text-xs text-[var(--fg-muted)] break-words">
                {mod.download
                  ? sourceLine(mod)
                  : mod.github_unread
                    ? `${mod.download_note ?? "GitHub was not read."} Download looks for it on GitHub now.`
                    : noDownloadLine(mod)}
              </p>
              {mod.compatibility_status &&
                NOTEWORTHY.has(mod.compatibility_status) && (
                  <p className="text-xs text-[var(--warning)] break-words">
                    Compatibility list: {mod.compatibility_status}
                    {mod.compatibility_summary
                      ? ` — ${mod.compatibility_summary}`
                      : ""}
                  </p>
                )}
              {rowError?.id === mod.profile_component_id && (
                <p
                  role="alert"
                  className="text-xs text-[var(--danger)] break-words"
                >
                  {errorSummary(rowError.error)}
                </p>
              )}
            </li>
          ))}
        </ul>
      )}

      {notes.length > 0 && (
        <div className="text-xs space-y-1">
          <p className="font-semibold">Compatibility notes</p>
          <ul className="list-disc pl-4">
            {notes.map((mod) => (
              <li key={mod.profile_component_id} className="break-words">
                {mod.name}: {mod.compatibility_status}
                {mod.compatibility_summary
                  ? ` — ${mod.compatibility_summary}`
                  : ""}
              </li>
            ))}
          </ul>
        </div>
      )}

      {unknown.length > 0 && (
        <details className="text-xs">
          <summary className="cursor-pointer text-[var(--fg-muted)]">
            Could not be checked ({unknown.length})
          </summary>
          <ul className="list-disc pl-4 mt-1">
            {unknown.map((mod) => (
              <li key={mod.profile_component_id} className="break-words">
                {mod.name}: {mod.errors.join(" ")}
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
};
