import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { useUpdateProfileDetails } from "@/shared/api/hooks";
import { errorSummary } from "@/shared/api/errors";
import type { ProfileSummaryDto } from "@/shared/api/generated";

/**
 * Renames a profile or changes its description. Only the label changes; the
 * profile's id, folders and history stay the same. The card shows the saved
 * value, never an unsaved edit.
 */
export const ProfileDetailsForm: React.FC<{
  profile: ProfileSummaryDto;
  onDone: () => void;
}> = ({ profile, onDone }) => {
  const [name, setName] = useState(profile.name);
  const [description, setDescription] = useState(profile.description ?? "");
  const [error, setError] = useState<string | null>(null);
  const update = useUpdateProfileDetails();

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    setError(null);
    try {
      await update.mutateAsync({
        profileId: profile.id,
        name,
        description: description.trim() ? description : null,
      });
      onDone();
    } catch (saveError) {
      setError(errorSummary(saveError, "Could not save the profile"));
    }
  };

  return (
    <form onSubmit={submit} className="space-y-2 text-xs">
      <label className="block space-y-1">
        <span className="font-medium">Name</span>
        <input
          type="text"
          value={name}
          maxLength={60}
          onChange={(event) => setName(event.target.value)}
          className="w-full px-3 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)]"
          autoFocus
        />
      </label>
      <label className="block space-y-1">
        <span className="font-medium">Description (optional)</span>
        <textarea
          value={description}
          maxLength={500}
          rows={2}
          onChange={(event) => setDescription(event.target.value)}
          className="w-full px-3 py-2 rounded-lg border border-[var(--border)] bg-[var(--bg-primary)]"
        />
      </label>
      {error && (
        <p role="alert" className="text-[var(--danger)]">
          {error}
        </p>
      )}
      <div className="flex gap-2">
        <Button
          type="submit"
          size="sm"
          variant="primary"
          disabled={!name.trim() || update.isPending}
          isLoading={update.isPending}
        >
          Save
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={onDone}>
          Cancel
        </Button>
      </div>
    </form>
  );
};
