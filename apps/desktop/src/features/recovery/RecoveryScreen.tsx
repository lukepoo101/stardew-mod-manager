import React from "react";
import type { RecoveryDetailDto } from "@/shared/api/generated";

const KIND: Record<string, string> = {
  SmapiSetup: "Setting up SMAPI",
  ModInstall: "Installing a mod",
  ModRemove: "Removing a mod",
  GameLaunch: "Starting the game",
  ProfileCreate: "Creating a profile",
  ProfileDelete: "Deleting a profile",
  ModFilesAccepted: "Accepting changed mod files",
  ModToggle: "Turning several mods on or off",
};

const STEP_STATE: Record<string, string> = {
  Completed: "finished",
  Running: "was running when it stopped",
  Failed: "failed",
  Pending: "not started",
};

/** "install_smapi_files" -> "Install smapi files". */
function stepName(kind: string): string {
  const words = kind.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/_/g, " ");
  return words.charAt(0).toUpperCase() + words.slice(1).toLowerCase();
}

/**
 * Shown at startup when an operation was interrupted and automatic recovery
 * could not settle it: what was happening, which steps are known to have
 * finished, and what can be done. The user may continue into the app; the
 * backend still refuses anything that claims the same installation or
 * profile.
 */
export const RecoveryScreen: React.FC<{
  summary: string;
  detail: RecoveryDetailDto | null;
  error: string | null;
  onRetry: () => void;
  /** Leaves the screen; conflicting actions stay refused by the backend. */
  onContinue?: () => void;
}> = ({ summary, detail, error, onRetry, onContinue }) => (
  <main className="p-8 space-y-4 max-w-2xl">
    <h1 className="text-xl font-bold">Recovery required</h1>
    <p role="alert">{error || summary}</p>
    {detail && (
      <section className="space-y-2 text-sm">
        <p>
          <span className="font-semibold">Interrupted: </span>
          {KIND[detail.kind] ?? detail.kind}, started{" "}
          {new Date(detail.started_at).toLocaleString()}.
        </p>
        {detail.steps.length > 0 ? (
          <div>
            <p className="font-semibold">What is known</p>
            <ol className="list-decimal pl-5">
              {detail.steps.map((step) => (
                <li key={step.step_index}>
                  {stepName(step.step_kind)}:{" "}
                  {STEP_STATE[step.state] ?? step.state}
                </li>
              ))}
            </ol>
          </div>
        ) : (
          <p>No step had been recorded as started.</p>
        )}
        {detail.error_code && (
          <details>
            <summary className="cursor-pointer">Technical details</summary>
            <p className="font-mono break-all select-text">
              {detail.operation_id} · {detail.state} · {detail.error_code}
              {detail.error_message ? ` · ${detail.error_message}` : ""}
            </p>
          </details>
        )}
      </section>
    )}
    <p className="text-sm">
      Retrying checks the files again and finishes or undoes the operation from
      what it finds. Running it more than once is safe. If it keeps failing, the
      record above is kept for troubleshooting.
    </p>
    <div className="flex gap-4">
      <button type="button" onClick={onRetry}>
        Retry recovery
      </button>
      {onContinue && (
        <button type="button" onClick={onContinue}>
          Continue to the app
        </button>
      )}
    </div>
    {onContinue && (
      <p className="text-xs">
        You can keep using the parts of the app this does not affect. Anything
        that would touch the same game installation or profile is refused until
        recovery finishes.
      </p>
    )}
  </main>
);
