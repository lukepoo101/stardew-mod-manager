import { useEffect, useState } from "react";
import { api } from "@/shared/api/client";
import type { BundleProgressDto } from "@/shared/api/generated";

/**
 * How far a change that installs many packages has got: importing a bundle,
 * duplicating a profile, finishing a copy, bringing a deleted profile back or
 * building one from a restore point. It is asked for while `active`; the
 * change runs in the background, so the answer comes at once.
 */
export function useInstallProgress(active: boolean): BundleProgressDto | null {
  const [progress, setProgress] = useState<BundleProgressDto | null>(null);
  useEffect(() => {
    if (!active) {
      setProgress(null);
      return;
    }
    let current = true;
    const ask = () =>
      Promise.resolve()
        .then(() => api.profileBundleProgress())
        .then((value) => {
          if (current) setProgress(value ?? null);
        })
        .catch(() => undefined);
    void ask();
    const timer = window.setInterval(() => void ask(), 400);
    return () => {
      current = false;
      window.clearInterval(timer);
    };
  }, [active]);
  return progress;
}

/** "Installing 3 of 15: Name", or `waiting` before the first package. */
export function installProgressLine(
  progress: BundleProgressDto | null,
  waiting: string,
): string {
  return progress && progress.total > 0
    ? `Installing ${Math.min(progress.done + 1, progress.total)} of ${progress.total}: ${progress.current}`
    : waiting;
}
