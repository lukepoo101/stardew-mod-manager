import React, { useId, useState } from "react";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";

const KEY = "smm.share-folder";

function remembered(): string {
  try {
    return localStorage.getItem(KEY) ?? "";
  } catch {
    return "";
  }
}

/**
 * The folder shared files (recipes, collection revisions, bundles) are
 * saved in, remembered on this computer so it only has to be given once.
 */
export function useShareFolder(): [string, (folder: string) => void] {
  const [folder, setFolder] = useState(remembered);
  const update = (next: string) => {
    setFolder(next);
    try {
      localStorage.setItem(KEY, next.trim());
    } catch {
      // Remembering the folder is a convenience; it still works this time.
    }
  };
  return [folder, update];
}

/**
 * Where a shared file goes: a path field anyone can type or paste into,
 * and a folder picker titled with what the folder is for.
 */
export const ShareFolderField: React.FC<{
  folder: string;
  onChange: (folder: string) => void;
  /** The native picker's title, such as "Choose where to save the bundle". */
  dialogTitle: string;
  disabled?: boolean;
}> = ({ folder, onChange, dialogTitle, disabled }) => {
  const id = useId();
  return (
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <label htmlFor={id} className="font-medium">
        Save in folder
      </label>
      <input
        id={id}
        value={folder}
        disabled={disabled}
        placeholder="For example ~/Documents"
        onChange={(event) => onChange(event.target.value)}
        className="flex-1 min-w-[12rem] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)] select-text"
      />
      <Button
        size="sm"
        variant="ghost"
        disabled={disabled}
        onClick={async () => {
          const chosen = await api.chooseFolderDialog(dialogTitle);
          if (chosen) onChange(chosen);
        }}
      >
        Choose folder...
      </Button>
    </div>
  );
};

/** Asks for a folder when none is given yet; `null` if the user cancels. */
export async function folderOrAsk(
  folder: string,
  dialogTitle: string,
  remember: (folder: string) => void,
): Promise<string | null> {
  if (folder.trim()) return folder.trim();
  const chosen = await api.chooseFolderDialog(dialogTitle);
  if (chosen) remember(chosen);
  return chosen;
}
