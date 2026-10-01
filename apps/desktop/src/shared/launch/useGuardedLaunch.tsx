import { useState } from "react";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { useLaunchGame } from "@/shared/api/hooks";
import { errorSummary } from "@/shared/api/errors";

type Mode = "Modded" | "Vanilla";

/**
 * Starts the game, first showing any non-blocking preflight warnings for the
 * user to accept. Accepting is recorded with that session only; the warnings
 * stay listed elsewhere until fixed. If they change before starting, the
 * backend refuses and the new list is shown to review again.
 */
export function useGuardedLaunch() {
  const launch = useLaunchGame();
  const [review, setReview] = useState<{
    mode: Mode;
    warnings: string[];
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
            setReview({ mode, warnings: preflight.warnings, changed: true });
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
      if (preflight.warnings.length > 0) {
        setReview({ mode, warnings: preflight.warnings, changed: false });
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
        Start with{" "}
        {review.warnings.length === 1 ? "this warning" : "these warnings"}?
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
