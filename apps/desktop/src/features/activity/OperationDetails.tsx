import React, { useState } from "react";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { OperationDetailsDto } from "@/shared/api/generated";

const CHANGE: Record<string, string> = {
  added: "Installed",
  removed: "Removed",
  profile_created: "Profile created",
};

/**
 * What one operation changed, loaded when opened. It shows the history as it
 * was recorded, not the mods' current details.
 */
export const OperationDetails: React.FC<{ operationId: string }> = ({
  operationId,
}) => {
  const [details, setDetails] = useState<OperationDetailsDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const load = async () => {
    if (loaded) return;
    setLoaded(true);
    try {
      setDetails(await api.getOperationHistoryDetails(operationId));
    } catch (loadError) {
      setError(errorSummary(loadError, "The details could not be loaded"));
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
