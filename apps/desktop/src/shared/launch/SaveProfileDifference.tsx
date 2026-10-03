import React from "react";
import { useProfileMods } from "@/shared/api/hooks";
import {
  compareProfiles,
  summarizeDifferences,
} from "@/shared/profiles/compare";

/**
 * How the profile about to start differs from the one the save belongs
 * with, so the warning says what is at stake. Read only.
 */
export const SaveProfileDifference: React.FC<{
  profileId: string;
  linkedProfileId: string;
  linkedProfileName: string;
}> = ({ profileId, linkedProfileId, linkedProfileName }) => {
  const { data: here } = useProfileMods(profileId);
  const { data: there } = useProfileMods(linkedProfileId);
  if (!here || !there) return null;
  const comparison = compareProfiles(here, there);
  const lines = summarizeDifferences(comparison);
  return (
    <div className="text-xs">
      <p className="font-semibold">
        Compared with "{linkedProfileName}" ({comparison.differences.length}{" "}
        difference(s))
      </p>
      {lines.length === 0 ? (
        <p>The two profiles have the same mods, versions and enabled state.</p>
      ) : (
        <ul className="list-disc pl-4">
          {lines.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      )}
    </div>
  );
};
