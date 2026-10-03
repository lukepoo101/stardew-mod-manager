import React, { useState } from "react";
import { Button } from "@/components/ui/Button";
import { errorSummary } from "@/shared/api/errors";
import type { SmapiStatusDto } from "@/shared/api/generated";
import { useSmapiStatus } from "@/shared/api/hooks";
import { compareVersions } from "@/shared/versions";
import { SmapiSetup } from "@/features/smapi/SmapiSetup";

type Choice = "adopt" | "update" | "keep";

/**
 * SMAPI is already in the game folder. By default the manager reinstalls
 * the same version so it knows exactly what is there and can repair, update
 * or roll it back later; updating to the suggested version, or leaving it
 * untouched, are the alternatives.
 */
export const ExistingSmapiChoice: React.FC<{
  gameId: string;
  onInstalled: (status: SmapiStatusDto) => void;
  onKeep: () => void;
  onBack: () => void;
}> = ({ gameId, onInstalled, onKeep, onBack }) => {
  const { data: status, error } = useSmapiStatus(gameId);
  const [choice, setChoice] = useState<Choice>("adopt");

  if (error)
    return (
      <p role="alert" className="text-xs text-[var(--danger)]">
        {errorSummary(error, "The SMAPI in the game folder could not be read")}
      </p>
    );
  if (!status)
    return (
      <p role="status" className="text-xs text-[var(--fg-muted)]">
        Reading the SMAPI in the game folder...
      </p>
    );

  const current = status.observed_version;
  const suggested = status.recommended_version;
  const newer =
    current && suggested ? compareVersions(suggested, current) > 0 : false;
  const options: { id: Choice; label: string; detail: string }[] = [
    {
      id: "adopt",
      label: current
        ? `Let the manager look after SMAPI ${current}`
        : "Let the manager look after SMAPI",
      detail: current
        ? `Reinstalls the same version from SMAPI's own download so the manager knows what is installed and can repair, update or roll it back. Your mods are not touched.`
        : "The installed version could not be read, so the suggested version is installed in its place.",
    },
    ...(newer
      ? [
          {
            id: "update" as const,
            label: `Update to SMAPI ${suggested}`,
            detail: `The newest SMAPI that supports ${status.game_version ? `Stardew Valley ${status.game_version}` : "your game"}, as SMAPI declares.`,
          },
        ]
      : []),
    {
      id: "keep",
      label: "Leave it as it is",
      detail:
        "Nothing in the game folder changes. The manager reports the SMAPI it finds but cannot roll it back.",
    },
  ];

  return (
    <div className="space-y-4 text-xs">
      <div className="space-y-1">
        <h3 className="font-bold text-sm">
          SMAPI {current ?? "(version unread)"} is already installed
        </h3>
        <p className="text-[var(--fg-muted)]">
          {status.game_version
            ? `Your game is Stardew Valley ${status.game_version}. `
            : ""}
          {status.installed_compatibility === "game_too_old"
            ? "SMAPI says this version needs a newer game."
            : status.installed_compatibility === "game_too_new"
              ? "SMAPI says this version does not support your game version; an update is needed."
              : status.installed_compatibility === "compatible"
                ? "SMAPI says this version supports your game."
                : ""}
        </p>
      </div>
      <fieldset className="space-y-2">
        <legend className="sr-only">What to do with the installed SMAPI</legend>
        {options.map((option) => (
          <label
            key={option.id}
            className="flex items-start gap-2 p-2 rounded-md border border-[var(--border)] cursor-pointer"
          >
            <input
              type="radio"
              name="existing-smapi"
              checked={choice === option.id}
              onChange={() => setChoice(option.id)}
            />
            <span>
              <span className="block font-semibold">{option.label}</span>
              <span className="block text-[var(--fg-muted)]">
                {option.detail}
              </span>
            </span>
          </label>
        ))}
      </fieldset>
      {choice === "keep" ? (
        <div className="flex justify-between items-center pt-1">
          <Button variant="ghost" onClick={onBack}>
            Back
          </Button>
          <Button variant="primary" onClick={onKeep}>
            Continue without changes
          </Button>
        </div>
      ) : (
        <SmapiSetup
          // A new choice starts a fresh preview and version.
          key={choice}
          gameId={gameId}
          initialVersion={
            choice === "adopt"
              ? (current ?? undefined)
              : (suggested ?? undefined)
          }
          installedVersion={current}
          actionLabel={choice === "adopt" ? "Reinstall SMAPI" : "Update SMAPI"}
          onInstalled={onInstalled}
          onBack={onBack}
        />
      )}
    </div>
  );
};
