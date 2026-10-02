import type { SmapiStatusDto } from "@/shared/api/generated";

export interface SmapiBadge {
  label: string;
  variant: "success" | "warning" | "danger" | "neutral" | "info";
}

/**
 * A short badge for the SMAPI in the game folder. It never shows the tested
 * version as if it were the installed one.
 */
export function smapiBadge(status: SmapiStatusDto | undefined): SmapiBadge {
  if (!status) return { label: "SMAPI not checked", variant: "neutral" };
  if (status.state === "absent")
    return { label: "No SMAPI", variant: "warning" };
  if (status.state === "partial")
    return { label: "SMAPI incomplete", variant: "danger" };
  const version = status.observed_version
    ? `SMAPI ${status.observed_version}`
    : "SMAPI (version unknown)";
  switch (status.comparison) {
    case "same":
      return { label: version, variant: "success" };
    case "newer":
      return { label: `${version}, newer than tested`, variant: "info" };
    case "older":
      return { label: `${version}, older than tested`, variant: "warning" };
    default:
      return { label: version, variant: "neutral" };
  }
}

/**
 * What the installed SMAPI means relative to the version this manager is
 * tested with, in plain words. "Tested" is not "latest": no update source is
 * consulted, and a newer SMAPI is never downgraded automatically.
 */
export function smapiExplanation(status: SmapiStatusDto): string {
  const tested = status.tested_version;
  switch (status.state === "installed" ? status.comparison : status.state) {
    case "absent":
      return `No SMAPI was found in the game folder. Setup installs SMAPI ${tested}, the version this manager is tested with.`;
    case "partial":
      return "Some SMAPI files are in the game folder and some are missing, so modded launches may fail. Running SMAPI's own installer again usually fixes this; the manager does not delete game files to repair it.";
    case "same":
      return `This is SMAPI ${tested}, the version this manager is tested with.`;
    case "newer":
      return `This is newer than SMAPI ${tested}, the version this manager is tested with. It has not been verified with this manager, which does not mean it is broken, and it will not be downgraded.`;
    case "older":
      return `This is older than SMAPI ${tested}, the version this manager is tested with. Some mods may need a newer SMAPI.`;
    default:
      return `SMAPI is installed but its version could not be read, so it cannot be compared with the tested version ${tested}.`;
  }
}
