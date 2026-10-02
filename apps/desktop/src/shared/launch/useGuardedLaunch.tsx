import { useState } from "react";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { useActiveProfileOverview, useLaunchGame } from "@/shared/api/hooks";
import { errorSummary } from "@/shared/api/errors";

type Mode = "Modded" | "Vanilla" | "RuntimeTest";

/**
 * Starts the game, first showing any non-blocking preflight warnings for the
 * user to accept. Accepting is recorded with that session only; the warnings
 * stay listed elsewhere until fixed. If they change before starting, the
 * backend refuses and the new list is shown to review again.
 */
/**
 * A note when the most recently played save is linked to another profile.
 * It is based on which save was played last, not on which one will be
 * loaded, and it is shown only; the backend does not check it.
 */
async function saveNotes(mode: Mode, profileId?: string): Promise<string[]> {
  if (mode !== "Modded" || !profileId) return [];
  try {
    const { saves } = await api.listSaves();
    const latest = saves
      .filter((save) => save.modified_at)
      .sort(
        (a, b) =>
          Date.parse(b.modified_at ?? "") - Date.parse(a.modified_at ?? ""),
      )[0];
    if (latest?.profile_id && latest.profile_id !== profileId) {
      return [
        `Your most recently played save, ${latest.farm_name ?? latest.id}, is linked to "${
          latest.profile_name ?? "another profile"
        }". Loading it with this profile's mods may change or break it.`,
      ];
    }
  } catch {
    // Saves that cannot be read add no note; the launch checks still run.
  }
  return [];
}

export function useGuardedLaunch() {
  const launch = useLaunchGame();
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const [review, setReview] = useState<{
    mode: Mode;
    warnings: string[];
    notes: string[];
    changed: boolean;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const start = (mode: Mode, acknowledged: string[]) => {
    setError(null);
    launch.mutate(
      { mode, acknowledged },
      {
        onSuccess: () => setReview(null),
        onError: async (launchError) => {
          const code = (launchError as { code?: string }).code;
          if (code === "LAUNCH_WARNINGS_CHANGED") {
            const preflight = await api.getLaunchPreflight(mode);
            setReview({
              mode,
              warnings: preflight.warnings,
              notes: await saveNotes(mode, profileId),
              changed: true,
            });
          } else {
            setReview(null);
            setError(errorSummary(launchError, "The game was not started"));
          }
        },
      },
    );
  };

  const request = async (mode: Mode) => {
    setError(null);
    try {
      const preflight = await api.getLaunchPreflight(mode);
      const notes = await saveNotes(mode, profileId);
      if (preflight.warnings.length > 0 || notes.length > 0) {
        setReview({
          mode,
          warnings: preflight.warnings,
          notes,
          changed: false,
        });
      } else {
        start(mode, []);
      }
    } catch (preflightError) {
      setError(errorSummary(preflightError, "The launch checks did not run"));
    }
  };

  const dialog = review ? (
    <Modal
      labelledBy="launch-warnings-title"
      onClose={launch.isPending ? undefined : () => setReview(null)}
      className="space-y-3 text-sm"
    >
      <h2 id="launch-warnings-title" className="text-lg font-bold">
        {review.warnings.length + review.notes.length === 1
          ? "Start with this warning?"
          : "Start with these warnings?"}
      </h2>
      {review.changed && (
        <p role="alert" className="text-xs text-[var(--warning)]">
          The warnings changed since you looked. Review them again.
        </p>
      )}
      <ul className="list-disc pl-4 text-xs space-y-1">
        {review.warnings.map((warning) => (
          <li key={warning}>{warning}</li>
        ))}
        {review.notes.map((note) => (
          <li key={note}>{note}</li>
        ))}
      </ul>
      <p className="text-xs text-[var(--fg-muted)]">
        None of these stops the game from starting. Starting anyway records that
        you saw them with this session; they stay listed until fixed.
      </p>
      <div className="flex justify-end gap-2">
        <Button
          type="button"
          variant="secondary"
          onClick={() => setReview(null)}
          disabled={launch.isPending}
        >
          Cancel
        </Button>
        <Button
          type="button"
          onClick={() => start(review.mode, review.warnings)}
          isLoading={launch.isPending}
          disabled={launch.isPending}
        >
          Start anyway
        </Button>
      </div>
    </Modal>
  ) : null;

  return { request, dialog, error, isPending: launch.isPending };
}
