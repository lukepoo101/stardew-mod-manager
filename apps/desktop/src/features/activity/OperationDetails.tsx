import React, { useState } from "react";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { OperationDetailsDto } from "@/shared/api/generated";
import { ErrorsAroundOperation } from "./ErrorsAroundOperation";

const CHANGE: Record<string, string> = {
  added: "Installed",
  removed: "Removed",
  profile_created: "Profile created",
  copied_from: "Copied from profile",
};

/**
 * What one operation changed, loaded when opened. It shows the history as it
 * was recorded, not the mods' current details.
 */
export const OperationDetails: React.FC<{
  operationId: string;
  /** Set for a finished removal, so it can be undone from the stored archive. */
  undoRemovalInto?: string | null;
  /** For a finished change to a profile: compare errors around it. */
  around?: { profileId: string; at: string } | null;
}> = ({ operationId, undoRemovalInto, around }) => {
  const [details, setDetails] = useState<OperationDetailsDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const [stored, setStored] = useState(false);
  const [undoStatus, setUndoStatus] = useState<string | null>(null);

  const load = async () => {
    if (loaded) return;
    setLoaded(true);
    try {
      const found = await api.getOperationHistoryDetails(operationId);
      setDetails(found);
      if (undoRemovalInto && found?.package_hash) {
        const hashes = await api.storedPackages([found.package_hash]);
        setStored(hashes.length > 0);
      }
    } catch (loadError) {
      setError(errorSummary(loadError, "The details could not be loaded"));
    }
  };

  const undo = async () => {
    if (!details?.package_hash || !undoRemovalInto) return;
    const names = details.changes
      .map((c) => [c.name, c.version].filter(Boolean).join(" "))
      .filter(Boolean)
      .join(", ");
    if (
      !window.confirm(
        `Install ${names || "these mods"} again${
          details.profile_name ? ` into ${details.profile_name}` : ""
        } from the stored archive? It is checked like any install, so it will not replace a version installed since. Settings the mod had before are not brought back.`,
      )
    )
      return;
    setUndoStatus(null);
    try {
      await api.installStoredPackage(undoRemovalInto, details.package_hash);
      setUndoStatus("Installed again.");
    } catch (undoError) {
      setUndoStatus(errorSummary(undoError, "It was not installed again"));
    }
  };

  return (
    <details
      className="text-xs mt-1"
      onToggle={(event) => {
        if ((event.target as HTMLDetailsElement).open) void load();
      }}
    >
      <summary className="cursor-pointer text-[var(--fg-muted)]">
        What changed
      </summary>
      {error && (
        <p role="alert" className="text-[var(--danger)]">
          {error}
        </p>
      )}
      {details && (
        <div className="mt-1 space-y-1">
          {details.profile_name && <p>Profile: {details.profile_name}</p>}
          {details.original_filename && (
            <p className="break-all">From: {details.original_filename}</p>
          )}
          {details.package_hash && (
            <p className="font-mono break-all text-[var(--fg-muted)]">
              SHA-256 {details.package_hash}
            </p>
          )}
          {details.folder && <p>Folder: {details.folder}</p>}
          {undoRemovalInto && details.package_hash && (
            <p>
              {stored ? (
                <button
                  type="button"
                  onClick={() => void undo()}
                  className="text-[var(--accent-primary)] hover:underline cursor-pointer"
                >
                  Install it again
                </button>
              ) : (
                <span className="text-[var(--fg-muted)]">
                  The archive is no longer stored, so this removal cannot be
                  undone here.
                </span>
              )}
            </p>
          )}
          {undoStatus && <p role="status">{undoStatus}</p>}
          {around && (
            <ErrorsAroundOperation
              profileId={around.profileId}
              at={around.at}
              changed={details.changes
                .map((c) => c.name)
                .filter((name): name is string => Boolean(name))}
            />
          )}
          {details.changes.length === 0 ? (
            <p className="text-[var(--fg-muted)]">No changes were recorded.</p>
          ) : (
            <ul className="list-disc pl-4">
              {details.changes.map((change, index) => (
                <li key={`${change.change}:${change.unique_id ?? index}`}>
                  {CHANGE[change.change] ?? change.change}:{" "}
                  {change.name ?? "name not recorded"}
                  {change.version ? ` ${change.version}` : ""}
                  {change.unique_id ? (
                    <span className="font-mono text-[var(--fg-muted)]">
                      {" "}
                      ({change.unique_id})
                    </span>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </details>
  );
};
