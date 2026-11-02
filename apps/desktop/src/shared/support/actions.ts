import { api } from "@/shared/api/client";
import { isTauri } from "@/shared/api/invoke";

/** Copies text and reports whether the browser allowed it. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** Offers text as a file download without touching the filesystem directly. */
export function downloadText(
  filename: string,
  text: string,
  mime = "application/json",
): void {
  const url = URL.createObjectURL(new Blob([text], { type: mime }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

/**
 * Saves text to a file. In the desktop app the user chooses where, and the
 * result is the path written (or null when they cancelled); in a browser it
 * is a normal download, whose location the browser decides.
 */
export async function saveTextFile(
  title: string,
  filename: string,
  text: string,
): Promise<{ saved: true; path: string | null } | { saved: false }> {
  if (!isTauri()) {
    downloadText(filename, text);
    return { saved: true, path: null };
  }
  const path = await api.saveTextFileDialog(title, filename, text);
  return path ? { saved: true, path } : { saved: false };
}
