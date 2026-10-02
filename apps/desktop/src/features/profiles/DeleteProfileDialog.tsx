import { Modal } from "@/components/ui/Modal";
import React, { useEffect, useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { ProfileDeletePreviewDto } from "@/shared/api/generated";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/**
 * Says exactly what deleting an archived profile removes and keeps, and asks
 * for the profile's name before doing it.
 */
export const DeleteProfileDialog: React.FC<{
  profileId: string;
  onClose: () => void;
}> = ({ profileId, onClose }) => {
  const [preview, setPreview] = useState<ProfileDeletePreviewDto | null>(null);
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .previewProfileDeletion(profileId)
      .then(setPreview)
      .catch((loadError) =>
        setError(errorSummary(loadError, "Could not check this profile")),
      );
  }, [profileId]);

  const remove = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.deleteProfile(profileId);
      onClose();
    } catch (deleteError) {
      setError(errorSummary(deleteError, "The profile was not deleted"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      labelledBy="delete-profile-title"
      onClose={busy ? undefined : onClose}
      className="space-y-3 text-sm"
    >
      <h2 id="delete-profile-title" className="text-lg font-bold">
        Delete {preview ? `"${preview.name}"` : "profile"} permanently?
      </h2>
      {preview && (
        <>
          <div className="text-xs space-y-1">
            <p className="font-semibold">Removed</p>
            <ul className="list-disc pl-4">
              <li>
                The profile and its {preview.mod_count} installed mod(s),
                including their settings ({formatBytes(preview.folder_bytes)}
                ).
              </li>
            </ul>
            <p className="font-semibold pt-1">Kept</p>
            <ul className="list-disc pl-4">
              <li>
                The {preview.packages_kept} downloaded package(s) it used. Other
                profiles may use them; Storage cleanup can remove unused ones.
              </li>
              <li>Your other profiles, SMAPI, the game and your saves.</li>
              <li>Its entries in Activity.</li>
            </ul>
            <p className="text-[var(--fg-muted)] pt-1">
              The profile's folder is moved to the manager's trash folder rather
              than erased at once. While it is there, you can bring the profile
              back from Recently deleted on this page; its mods come back from
              the stored archives and its settings from the folder.
            </p>
          </div>
          {preview.blocked_reason ? (
            <p role="alert" className="text-xs text-[var(--warning)]">
              {preview.blocked_reason}
            </p>
          ) : (
            <label className="block text-xs space-y-1">
              <span>
                Type <strong>{preview.name}</strong> to confirm
              </span>
              <input
                type="text"
                value={typed}
                onChange={(event) => setTyped(event.target.value)}
                className="w-full px-3 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)]"
              />
            </label>
          )}
        </>
      )}
      {error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {error}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button variant="secondary" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="danger"
          disabled={
            busy ||
            !preview ||
            preview.blocked_reason !== null ||
            typed.trim() !== preview.name
          }
          isLoading={busy}
          onClick={remove}
        >
          Delete permanently
        </Button>
      </div>
    </Modal>
  );
};
