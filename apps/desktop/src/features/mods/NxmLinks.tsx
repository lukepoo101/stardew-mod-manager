import React, { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useNexusStatus, useNxmLinks } from "@/shared/api/hooks";
import type { DownloadedPreviewDto, NxmLinkDto } from "@/shared/api/generated";

/** "file 12345 of Nexus mod 541", or the installed mod it updates. */
function linkTitle(link: NxmLinkDto): string {
  return link.installed_name
    ? `A file for ${link.installed_name} (Nexus mod ${link.mod_id}, file ${link.file_id})`
    : `Nexus mod ${link.mod_id}, file ${link.file_id}`;
}

/**
 * Files the user chose with Nexus Mods' "Mod manager download" button,
 * waiting to be downloaded into this profile. Each is downloaded only when
 * the user asks, and goes to the same review as a chosen archive. The link's
 * download key stays in the backend.
 */
export const NxmLinks: React.FC<{
  profileId: string;
  /** Busy elsewhere (a review is open or committing). */
  disabled?: boolean;
  onDownloaded: (downloaded: DownloadedPreviewDto) => void;
}> = ({ profileId, disabled = false, onDownloaded }) => {
  const { data } = useNxmLinks();
  const { data: nexus } = useNexusStatus();
  const queryClient = useQueryClient();
  const [preparing, setPreparing] = useState<string | null>(null);
  const [rowError, setRowError] = useState<{ id: string; error: unknown }>();

  const links = data?.links ?? [];
  const refused = data?.refused ?? [];
  // A link is used up by a download, even one that failed, so its error is
  // shown on its own once the row has gone.
  const orphanError =
    rowError && !links.some((l) => l.id === rowError.id) ? rowError : null;
  if (links.length === 0 && refused.length === 0 && !orphanError) return null;

  const refresh = () => queryClient.refetchQueries({ queryKey: ["nxm-links"] });
  const connected = Boolean(nexus?.key_set) && !nexus?.rejected;

  const download = async (link: NxmLinkDto) => {
    setPreparing(link.id);
    setRowError(undefined);
    try {
      onDownloaded(await api.prepareNxmDownload(profileId, link.id));
    } catch (error) {
      setRowError({ id: link.id, error });
    } finally {
      setPreparing(null);
      void refresh();
    }
  };

  const dismiss = async (link: NxmLinkDto) => {
    setRowError(undefined);
    await api.dismissNxmLink(link.id);
    void refresh();
  };

  return (
    <section
      aria-labelledby="nxm-links-title"
      className="p-4 rounded-xl border border-[var(--border)] bg-[var(--bg-surface)] space-y-3 text-sm"
    >
      <div>
        <h3 id="nxm-links-title" className="font-semibold">
          From Nexus Mods
        </h3>
        <p className="text-xs text-[var(--fg-muted)]">
          Files you chose with "Mod manager download" on Nexus Mods. Nothing is
          downloaded until you ask, and each is reviewed before it is installed.
        </p>
      </div>
      {!connected && links.length > 0 && (
        <p role="status" className="text-xs">
          {nexus?.personal_key_allowed
            ? "Add your Nexus Mods API key in Settings to download these."
            : "This build of the manager is not connected to Nexus Mods yet. Open the mod page and download the file there, then install the archive here."}
        </p>
      )}
      {orphanError && (
        <p role="alert" className="text-xs text-[var(--danger)] break-words">
          {errorSummary(orphanError.error)}
        </p>
      )}
      {refused.length > 0 && (
        <div
          role="alert"
          className="text-xs text-[var(--warning)] p-2 rounded-lg bg-[var(--warning-surface)] space-y-1"
        >
          <ul className="list-disc pl-4">
            {refused.map((why, index) => (
              // The same reason can repeat; the order is the order received.
              <li key={`${index}:${why}`}>{why}</li>
            ))}
          </ul>
          <Button
            size="sm"
            variant="ghost"
            onClick={() =>
              void api.clearRefusedNxmLinks().then(() => void refresh())
            }
          >
            Dismiss
          </Button>
        </div>
      )}
      {links.length > 0 && (
        <ul className="space-y-2" aria-label="Nexus Mods files waiting">
          {links.map((link) => (
            <li
              key={link.id}
              className="p-3 rounded-lg border border-[var(--border)] space-y-1"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="min-w-0 break-words font-medium">
                  {linkTitle(link)}
                </span>
                <span className="flex gap-2">
                  {connected && (
                    <Button
                      size="sm"
                      variant="primary"
                      isLoading={preparing === link.id}
                      disabled={disabled || preparing !== null}
                      onClick={() => void download(link)}
                    >
                      Download and review
                    </Button>
                  )}
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => void api.openExternalPage(link.page_url)}
                  >
                    Open mod page
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Dismiss ${linkTitle(link)}`}
                    disabled={preparing === link.id}
                    onClick={() => void dismiss(link)}
                  >
                    Dismiss
                  </Button>
                </span>
              </div>
              {link.expires_at && (
                <p className="text-xs text-[var(--fg-muted)]">
                  The link works until{" "}
                  {new Date(link.expires_at).toLocaleTimeString()}.
                </p>
              )}
              {rowError?.id === link.id && (
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
    </section>
  );
};
