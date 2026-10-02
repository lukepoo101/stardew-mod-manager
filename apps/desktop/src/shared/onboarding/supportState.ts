/**
 * What a game folder's inspection result means, in words, and what to do
 * about it. The backend decides the state; this only explains it.
 */
export interface SupportStateText {
  label: string;
  explanation: string;
  /** A concrete next step, or null when the folder can be used as it is. */
  next: string | null;
  tone: "success" | "warning" | "danger";
}

const STATES: Record<string, SupportStateText> = {
  supported_fresh: {
    label: "Ready",
    explanation: "A Stardew Valley installation with no mods or SMAPI yet.",
    next: null,
    tone: "success",
  },
  supported_managed: {
    label: "Already set up",
    explanation: "This installation is already set up in the manager.",
    next: null,
    tone: "success",
  },
  existing_modded_unmanaged: {
    label: "Already modded",
    explanation:
      "This installation seems to have SMAPI or mods that were set up outside the manager. Taking over an existing setup is not supported yet. Nothing here has been changed.",
    next: "Choose another installation, or continue without letting the manager change this one.",
    tone: "warning",
  },
  unsupported_platform: {
    label: "Other platform",
    explanation:
      "This looks like a Stardew Valley installation for a different operating system, such as a Windows copy under Proton. This build cannot manage it.",
    next: "Choose the installation made for this computer's operating system.",
    tone: "danger",
  },
  invalid_game_directory: {
    label: "Not the game folder",
    explanation:
      "The folder is missing, or the game's files were not found in it. It may be a parent or child of the right folder.",
    next: "Choose the folder that contains the Stardew Valley launcher, usually named 'Stardew Valley'.",
    tone: "danger",
  },
  unreadable: {
    label: "Cannot be read",
    explanation:
      "The manager is not allowed to look inside this folder, so it cannot tell whether it is the game.",
    next: "Check the folder's permissions, or choose another folder.",
    tone: "danger",
  },
  unwritable: {
    label: "Read-only",
    explanation:
      "This is the game, but the manager cannot write to it, which SMAPI setup needs.",
    next: "Make the folder writable for your user, or move the game somewhere you own.",
    tone: "danger",
  },
};

export function describeSupportState(state: string): SupportStateText {
  return (
    STATES[state] ?? {
      label: "Unclear",
      explanation:
        "The checks could not tell what this folder is. That is not the same as it being broken.",
      next: "Choose another folder, or look at the details below.",
      tone: "warning",
    }
  );
}
