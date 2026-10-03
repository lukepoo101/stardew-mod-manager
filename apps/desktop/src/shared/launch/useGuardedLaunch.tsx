import { SaveProfileDifference } from "./SaveProfileDifference";
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
interface SaveMismatch {
  note: string;
  saveId: string;
  linkedProfileId: string;
  linkedProfileName: string;
}

const ACCEPTED_KEY = "smm-accepted-save-profile-pairs";

/**
 * Save/profile pairs the user chose to stop being warned about. Each is
 * keyed by the active profile's revision, so a changed profile warns again.
 */
function acceptedPairs(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(ACCEPTED_KEY) ?? "[]"));
  } catch {
    return new Set();
  }
}

function acceptPair(key: string): void {
  try {
    const pairs = acceptedPairs();
    pairs.add(key);
    localStorage.setItem(ACCEPTED_KEY, JSON.stringify([...pairs].slice(-50)));
  } catch {
    // Without storage the warning simply shows again next time.
  }
}

const pairKey = (saveId: string, profileId: string, revision: number) =>
  `${saveId}|${profileId}|${revision}`;

async function saveNotes(
  mode: Mode,
  profileId?: string,
  revision?: number,
): Promise<SaveMismatch | null> {
  if (mode !== "Modded" || !profileId) return null;
  try {
    const { saves } = await api.listSaves();
    const latest = saves
      .filter((save) => save.modified_at)
      .sort(
        (a, b) =>
          Date.parse(b.modified_at ?? "") - Date.parse(a.modified_at ?? ""),
      )[0];
    if (
      latest?.profile_id &&
      latest.profile_id !== profileId &&
      !acceptedPairs().has(pairKey(latest.id, profileId, revision ?? 0))
    ) {
      return {
        note: `Your most recently played save, ${latest.farm_name ?? latest.id}, is linked to "${
          latest.profile_name ?? "another profile"
        }". Loading it with this profile's mods may change or break it.`,
        saveId: latest.id,
        linkedProfileId: latest.profile_id,
        linkedProfileName: latest.profile_name ?? "the linked profile",
      };
    }
  } catch {
    // Saves that cannot be read add no note; the launch checks still run.
  }
  return null;
}

export function useGuardedLaunch() {
  const launch = useLaunchGame();
  const { data: overview } = useActiveProfileOverview();
  const profileId = overview?.profile.id;
  const revision = overview?.profile.revision;
  const [review, setReview] = useState<{
    mode: Mode;
    warnings: string[];
    notes: string[];
    mismatch: SaveMismatch | null;
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
            const mismatch = await saveNotes(mode, profileId, revision);
            setReview({
              mode,
              warnings: preflight.warnings,
              notes: mismatch ? [mismatch.note] : [],
              mismatch,
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
      const mismatch = await saveNotes(mode, profileId, revision);
      const notes = mismatch ? [mismatch.note] : [];
      if (preflight.warnings.length > 0 || notes.length > 0) {
        setReview({
          mode,
          warnings: preflight.warnings,
          notes,
          mismatch,
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
      {review.mismatch && profileId && (
        <SaveProfileDifference
          profileId={profileId}
          linkedProfileId={review.mismatch.linkedProfileId}
          linkedProfileName={review.mismatch.linkedProfileName}
        />
      )}
      {review.mismatch && profileId && (
        <div className="text-xs flex flex-wrap gap-3">
          <button
            type="button"
            className="underline cursor-pointer"
            disabled={launch.isPending}
            onClick={async () => {
              const mismatch = review.mismatch;
              if (!mismatch) return;
              setReview(null);
              try {
                await api.activateProfile(mismatch.linkedProfileId);
              } catch (switchError) {
                setError(
                  errorSummary(switchError, "The profile was not switched"),
                );
              }
            }}
          >
            Switch to "{review.mismatch.linkedProfileName}" instead
          </button>
          <button
            type="button"
            className="underline cursor-pointer text-[var(--fg-muted)]"
            disabled={launch.isPending}
            onClick={() => {
              const mismatch = review.mismatch;
              if (!mismatch) return;
              acceptPair(pairKey(mismatch.saveId, profileId, revision ?? 0));
              setReview({
                ...review,
                mismatch: null,
                notes: [],
              });
            }}
          >
            Don't warn about this save with this profile until it changes
          </button>
        </div>
      )}
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
