import { Modal } from "@/components/ui/Modal";
import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type {
  BundleImportDto,
  ProfileSummaryDto,
} from "@/shared/api/generated";

/**
 * Duplicates a profile into an independent copy with its own folder. The
 * source is not changed and stays active; the copy can be activated later.
 */
export const CloneProfileDialog: React.FC<{
  profile: ProfileSummaryDto;
  onClose: () => void;
}> = ({ profile, onClose }) => {
  const [name, setName] = useState(`${profile.name} copy`);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<BundleImportDto | null>(null);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      setResult(await api.cloneProfile(profile.id, name));
    } catch (cloneError) {
      setError(errorSummary(cloneError, "The profile was not duplicated"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      labelledBy="clone-profile-title"
      onClose={busy ? undefined : onClose}
      className="space-y-3 text-sm"
    >
      <h2 id="clone-profile-title" className="text-lg font-bold">
        Duplicate "{profile.name}"
      </h2>
      {result ? (
        <div className="text-xs space-y-2" role="status">
          <p>
            Created "{result.profile_name}" with {result.installed.length}{" "}
            mod(s)
            {result.disabled.length > 0
              ? `, ${result.disabled.length} left disabled as in the original`
              : ""}
            . "{profile.name}" is unchanged.
          </p>
          {result.failures.length > 0 && (
            <div>
              <p className="font-semibold text-[var(--warning)]">Not copied</p>
              <ul className="list-disc pl-4">
                {result.failures.map((failure) => (
                  <li key={`${failure.name}:${failure.reason}`}>
                    {failure.name}: {failure.reason}
                  </li>
                ))}
              </ul>
            </div>
          )}
          <div className="flex justify-end">
            <Button size="sm" onClick={onClose}>
              Done
            </Button>
          </div>
        </div>
      ) : (
        <form onSubmit={submit} className="space-y-3 text-xs">
          <p className="text-[var(--fg-muted)]">
            The copy gets its own folder and the same mods, versions and enabled
            state, installed from the packages the manager kept. Changing one
            never changes the other.
          </p>
          <div>
            <p className="font-medium">What is copied from "{profile.name}"</p>
            <ul className="list-disc pl-4 text-[var(--fg-muted)]">
              <li>
                {profile.mod_count} mod(s) at their installed versions, enabled
                or disabled as they are now
              </li>
              <li>Each mod's settings files (config.json)</li>
              <li>The description, starting as "Copy of {profile.name}"</li>
            </ul>
            <p className="text-[var(--fg-muted)] mt-1">
              Not copied: notes, freezes, restore points, save associations or
              the default/active status. Activity records which profile it was
              copied from.
            </p>
          </div>
          <label className="block space-y-1">
            <span className="font-medium">Name for the copy</span>
            <input
              type="text"
              value={name}
              maxLength={60}
              onChange={(event) => setName(event.target.value)}
              className="w-full px-3 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)]"
              autoFocus
            />
          </label>
          {error && (
            <p role="alert" className="text-[var(--danger)]">
              {error}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button type="button" variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={busy || !name.trim()}
              isLoading={busy}
            >
              Duplicate
            </Button>
          </div>
        </form>
      )}
    </Modal>
  );
};
