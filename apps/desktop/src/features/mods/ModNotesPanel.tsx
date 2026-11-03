import { SettingsBackups } from "./SettingsBackups";
import React, { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type {
  ModAnnotationDto,
  ModSizeDto,
  ReinstallPreviewDto,
} from "@/shared/api/generated";
import { ReinstallReview, reinstallSummary } from "./ReinstallReview";
import { formatBytes } from "@/features/settings/StorageCleanupCard";
import { parseTagInput } from "@/shared/mods/organise";
import { FolderOpen, Archive, RotateCcw } from "lucide-react";

/**
 * The user's own tags and note for one mod, plus shortcuts to its files. Tags
 * and notes follow the mod's UniqueID into every profile and never affect
 * what is installed or loaded.
 */
export const ModNotesPanel: React.FC<{
  profileComponentId: string;
  uniqueId: string;
  annotation: ModAnnotationDto | undefined;
  /** The installed package's checksum, to tell whether it is still stored. */
  artifactHash?: string;
  /** The profile the mod is in, for its settings backups. */
  profileId?: string;
}> = ({
  profileComponentId,
  uniqueId,
  annotation,
  artifactHash,
  profileId,
}) => {
  const [tags, setTags] = useState((annotation?.tags ?? []).join(", "));
  const [note, setNote] = useState(annotation?.note ?? "");
  const [status, setStatus] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [sourceUrl, setSourceUrl] = useState(annotation?.source_url ?? "");
  useEffect(() => {
    setSourceUrl(annotation?.source_url ?? "");
  }, [annotation?.source_url]);

  const saveSource = async (url: string | null) => {
    setStatus(null);
    try {
      await api.setModSourceLink(uniqueId, url);
      setStatus(url ? "Source link saved." : "Source link removed.");
    } catch (error) {
      setStatus(errorSummary(error, "The source link was not saved"));
    }
  };

  useEffect(() => {
    setTags((annotation?.tags ?? []).join(", "));
    setNote(annotation?.note ?? "");
  }, [annotation]);

  const [size, setSize] = useState<ModSizeDto | null>(null);
  useEffect(() => {
    let current = true;
    api
      .getModSize(profileComponentId)
      .then((measured) => {
        if (current) setSize(measured);
      })
      .catch(() => {
        if (current) setSize(null);
      });
    return () => {
      current = false;
    };
  }, [profileComponentId]);

  // Unknown (undefined) until checked; the archive actions only show when the
  // package is still stored.
  const [packageStored, setPackageStored] = useState<boolean | undefined>(
    artifactHash ? undefined : true,
  );
  useEffect(() => {
    if (!artifactHash) return;
    let current = true;
    api
      .storedPackages([artifactHash])
      .then((stored) => {
        if (current) setPackageStored(stored.length > 0);
      })
      .catch(() => {
        if (current) setPackageStored(true);
      });
    return () => {
      current = false;
    };
  }, [artifactHash]);

  const save = async () => {
    setSaving(true);
    setStatus(null);
    try {
      await api.setModAnnotation({
        unique_id: uniqueId,
        favourite: annotation?.favourite ?? false,
        tags: parseTagInput(tags),
        note,
      });
      setStatus("Saved.");
    } catch (error) {
      setStatus(errorSummary(error, "Could not save"));
    } finally {
      setSaving(false);
    }
  };

  // A reinstall shows what it will do with added files before it starts.
  const [review, setReview] = useState<ReinstallPreviewDto | null>(null);
  const [reinstalling, setReinstalling] = useState(false);
  const reinstall = async () => {
    setStatus(null);
    try {
      setReview(await api.previewReinstall(profileComponentId));
    } catch (error) {
      setStatus(errorSummary(error, "The mod was not reinstalled"));
    }
  };
  const confirmReinstall = async (keep: string[]) => {
    setReinstalling(true);
    try {
      const result = await api.reinstallMod(profileComponentId, keep);
      setReview(null);
      setStatus(reinstallSummary(result));
    } catch (error) {
      setStatus(errorSummary(error, "The mod was not reinstalled"));
    } finally {
      setReinstalling(false);
    }
  };

  const reveal = async (which: "files" | "package") => {
    setStatus(null);
    try {
      if (which === "files") await api.revealModFiles(profileComponentId);
      else await api.revealModPackage(profileComponentId);
    } catch (error) {
      setStatus(errorSummary(error, "Could not open the file manager"));
    }
  };

  return (
    <div className="space-y-3">
      {size && (
        <p className="text-xs text-[var(--fg-muted)]">
          Folder{" "}
          {size.folder_bytes === null
            ? "size could not be read"
            : formatBytes(size.folder_bytes)}
          {size.archive_bytes !== null &&
            ` · stored archive ${formatBytes(size.archive_bytes)}`}
        </p>
      )}
      <p className="text-xs text-[var(--fg-muted)]">
        Editing files in the mod folder yourself counts as an outside change:
        Check mod files on the Diagnostics page will list it, and reinstalling
        or changing the version replaces those files (settings are kept).
      </p>
      {packageStored === false && (
        <p className="text-xs text-[var(--fg-muted)]">
          The archive this mod came from is no longer stored, so it cannot be
          shown or used to reinstall. Install the file again to keep a copy.
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="secondary"
          onClick={() => reveal("files")}
          className="flex items-center gap-1.5"
        >
          <FolderOpen className="w-3.5 h-3.5" />
          <span>Show mod folder</span>
        </Button>
        {packageStored !== false && (
          <Button
            size="sm"
            variant="secondary"
            onClick={() => reveal("package")}
            className="flex items-center gap-1.5"
          >
            <Archive className="w-3.5 h-3.5" />
            <span>Show original archive</span>
          </Button>
        )}
        <Button
          size="sm"
          variant="secondary"
          onClick={reinstall}
          disabled={packageStored === false}
          title={
            packageStored === false
              ? "The archive this mod came from is no longer stored"
              : undefined
          }
          className="flex items-center gap-1.5"
        >
          <RotateCcw className="w-3.5 h-3.5" />
          <span>Reinstall from archive</span>
        </Button>
      </div>
      {review && (
        <ReinstallReview
          preview={review}
          busy={reinstalling}
          onConfirm={(keep) => void confirmReinstall(keep)}
          onCancel={() => setReview(null)}
        />
      )}

      {profileId && (
        <SettingsBackups profileId={profileId} uniqueId={uniqueId} />
      )}

      {uniqueId ? (
        <div className="space-y-2">
          <h4 className="text-xs font-bold text-[var(--fg-muted)] uppercase tracking-wider">
            Your tags and notes
          </h4>
          <label className="block text-xs space-y-1">
            <span className="font-medium">Tags, separated by commas</span>
            <input
              type="text"
              value={tags}
              onChange={(event) => setTags(event.target.value)}
              placeholder="e.g. visuals, needs config"
              className="w-full px-3 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)] text-xs"
            />
          </label>
          <label className="block text-xs space-y-1">
            <span className="font-medium">Note</span>
            <textarea
              value={note}
              onChange={(event) => setNote(event.target.value)}
              rows={3}
              maxLength={4000}
              className="w-full px-3 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)] text-xs"
            />
          </label>
          <p className="text-xs text-[var(--fg-muted)]">
            Kept on this computer for every profile with this mod. They never
            change what is installed or loaded.
          </p>
          <Button size="sm" variant="primary" onClick={save} isLoading={saving}>
            Save tags and note
          </Button>
          <div className="space-y-1 pt-2">
            <label className="block text-xs space-y-1">
              <span className="font-medium">
                Where you got it (your own link)
              </span>
              <input
                type="url"
                value={sourceUrl}
                onChange={(event) => setSourceUrl(event.target.value)}
                placeholder="https://..."
                className="w-full px-3 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)] text-xs"
              />
            </label>
            {annotation?.source_url && (
              <p className="text-xs text-[var(--fg-muted)]">
                Added by you
                {annotation.source_added_at
                  ? ` on ${new Date(annotation.source_added_at).toLocaleDateString()}`
                  : ""}
                . It is a note for you; how the file was actually added is kept
                separately and is not changed.
              </p>
            )}
            <div className="flex gap-2">
              <Button
                size="sm"
                variant="secondary"
                disabled={!sourceUrl.trim()}
                onClick={() => void saveSource(sourceUrl.trim())}
              >
                Save link
              </Button>
              {annotation?.source_url && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void saveSource(null)}
                >
                  Remove link
                </Button>
              )}
            </div>
          </div>
        </div>
      ) : (
        <p className="text-xs text-[var(--fg-muted)]">
          Tags and notes need a UniqueID, and this mod's manifest has none.
        </p>
      )}
      {status && (
        <p role="status" className="text-xs text-[var(--fg-muted)]">
          {status}
        </p>
      )}
    </div>
  );
};
