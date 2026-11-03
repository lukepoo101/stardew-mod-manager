import type { ModFilesCheckDto } from "@/shared/api/generated";

const list = (files: string[]) =>
  files.length > 3
    ? `${files.slice(0, 3).join(", ")} and ${files.length - 3} more`
    : files.join(", ");

/**
 * Mod folders whose files no longer match what was installed, in words, for
 * the launch checks. Settings edits and changes the user accepted are left
 * out: they are normal. These never block a launch; a missing DLL only means
 * SMAPI will probably not load that mod.
 */
export function fileNotes(checks: ModFilesCheckDto[]): string[] {
  const notes: string[] = [];
  for (const check of checks) {
    const name = check.mods.length > 0 ? check.mods.join(", ") : "A mod";
    if (check.status === "missing_folder") {
      notes.push(
        `${name}: its folder is missing from the profile, so SMAPI will not load it.`,
      );
    } else if (check.status === "changed" && check.missing.length > 0) {
      notes.push(
        `${name}: ${check.missing.length} installed file(s) are missing (${list(
          check.missing,
        )}), so SMAPI may not load it.`,
      );
    } else if (check.status === "changed" && check.modified.length > 0) {
      notes.push(
        `${name}: ${check.modified.length} file(s) changed since it was installed (${list(
          check.modified,
        )}).`,
      );
    }
  }
  return notes;
}
