/**
 * Web links in curator notes. Notes are plain text and are never rendered as
 * markup; links are offered for copying, so nothing opens on its own.
 */
export function linksIn(text: string): string[] {
  const found = text.match(/https?:\/\/[^\s<>"')\]]+/g) ?? [];
  return [...new Set(found.map((link) => link.replace(/[.,;:!?]+$/, "")))];
}

/** The curator's notes recorded in a published revision's recipe, if any. */
export function revisionNotes(recipeJson: string): string {
  try {
    const notes = JSON.parse(recipeJson)?.collection?.notes;
    return typeof notes === "string" ? notes : "";
  } catch {
    return "";
  }
}
