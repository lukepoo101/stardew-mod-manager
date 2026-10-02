import React, { useState } from "react";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type {
  ModRequirementDto,
  StoredCandidateDto,
} from "@/shared/api/generated";

/**
 * Offers to install a missing requirement from a package the manager
 * already stores, after saying which one, which version and why. With no
 * stored copy it says so and gives the UniqueID to look for, rather than
 * guessing a download.
 */
export const StoredDependencyInstall: React.FC<{
  requirement: ModRequirementDto;
  requiredBy: string;
  profileId: string;
}> = ({ requirement, requiredBy, profileId }) => {
  const [candidates, setCandidates] = useState<StoredCandidateDto[] | null>(
    null,
  );
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const look = async () => {
    setMessage(null);
    try {
      setCandidates(
        await api.findStoredMod(
          requirement.unique_id,
          requirement.minimum_version ?? null,
        ),
      );
    } catch (error) {
      setMessage(errorSummary(error, "Stored copies could not be listed"));
    }
  };

  const install = async (candidate: StoredCandidateDto) => {
    setBusy(true);
    setMessage(null);
    try {
      await api.installStoredPackage(profileId, candidate.artifact_hash, true);
      setMessage(`Installed ${candidate.name} ${candidate.version}.`);
      setCandidates([]);
    } catch (error) {
      setMessage(errorSummary(error, "It was not installed"));
    } finally {
      setBusy(false);
    }
  };

  if (candidates === null) {
    return (
      <button
        type="button"
        className="text-[var(--accent-primary)] hover:underline cursor-pointer"
        onClick={() => void look()}
      >
        Find a stored copy
      </button>
    );
  }
  const usable = candidates.filter((c) => c.meets_minimum !== false);
  return (
    <span className="block space-y-1">
      {message && <span className="block">{message}</span>}
      {!message && usable.length === 0 && (
        <span className="block text-[var(--fg-muted)]">
          No stored copy
          {candidates.length > 0 ? " new enough" : ""}. Look for{" "}
          <span className="font-mono">{requirement.unique_id}</span>
          {requirement.minimum_version
            ? ` ${requirement.minimum_version} or newer`
            : ""}{" "}
          on its mod page and install the download.
        </span>
      )}
      {usable.map((candidate) => (
        <span key={candidate.artifact_hash} className="block">
          {candidate.name} {candidate.version} from{" "}
          {candidate.original_filename}, stored by the manager, required by{" "}
          {requiredBy}
          {candidate.meets_minimum === null
            ? ". Its version could not be compared with the requirement"
            : ""}
          .{" "}
          <button
            type="button"
            disabled={busy}
            className="text-[var(--accent-primary)] hover:underline cursor-pointer"
            onClick={() => void install(candidate)}
          >
            Install it
          </button>
        </span>
      ))}
    </span>
  );
};
