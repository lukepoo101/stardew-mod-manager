import type { SmapiStatusDto } from "@/shared/api/generated";

export interface SmapiBadge {
  label: string;
  variant: "success" | "warning" | "danger" | "neutral" | "info";
}

/** A short badge for the SMAPI in the game folder and how it fits the game. */
export function smapiBadge(status: SmapiStatusDto | undefined): SmapiBadge {
  if (!status) return { label: "SMAPI not checked", variant: "neutral" };
  if (status.state === "absent")
    return { label: "No SMAPI", variant: "warning" };
  if (status.state === "partial")
    return { label: "SMAPI incomplete", variant: "danger" };
  const version = status.observed_version
    ? `SMAPI ${status.observed_version}`
    : "SMAPI (version unknown)";
  if (
    status.installed_compatibility === "game_too_old" ||
    status.installed_compatibility === "game_too_new"
  )
    return { label: `${version}, not for this game`, variant: "danger" };
  if (status.update_available)
    return { label: `${version}, update available`, variant: "info" };
  if (status.installed_compatibility === "compatible")
    return { label: version, variant: "success" };
  return { label: version, variant: "neutral" };
}

/**
 * What the installed SMAPI means for this game, in plain words. Which game
 * versions a SMAPI supports is what SMAPI itself declares.
 */
export function smapiExplanation(status: SmapiStatusDto): string {
  const game = status.game_version
    ? `Stardew Valley ${status.game_version}`
    : "this game (its version could not be read)";
  const suggested = status.recommended_version;
  if (status.state === "absent")
    return suggested
      ? `No SMAPI was found in the game folder. SMAPI ${suggested} is suggested for ${game}.`
      : `No SMAPI was found in the game folder, and no SMAPI release is known to support ${game}.`;
  if (status.state === "partial")
    return "Some SMAPI files are in the game folder and some are missing, so modded launches may fail. Running SMAPI's own installer again usually fixes this; the manager does not delete game files to repair it.";
  const installed = status.observed_version
    ? `SMAPI ${status.observed_version}`
    : "This SMAPI";
  switch (status.installed_compatibility) {
    case "game_too_old":
      return `${installed} needs a newer game than ${game}. Update the game, or choose an older SMAPI that supports it.`;
    case "game_too_new":
      return `${installed} does not support a game as new as ${game}. Update SMAPI before playing modded.`;
    case "compatible":
      return status.update_available
        ? `${installed} supports ${game}. SMAPI ${suggested} is newer and also supports it.`
        : `${installed} supports ${game}, as SMAPI declares.`;
    default:
      return status.observed_version
        ? `Whether ${installed} supports ${game} could not be told. Refresh the SMAPI release list to check.`
        : "SMAPI is installed but its version could not be read.";
  }
}
