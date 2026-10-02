/** Top-level manifest.json fields this manager reads (case-insensitive). */
const READ_FIELDS = new Set(
  [
    "Name",
    "Author",
    "Version",
    "Description",
    "UniqueID",
    "EntryDll",
    "ContentPackFor",
    "MinimumApiVersion",
    "MinimumGameVersion",
    "Dependencies",
    "UpdateKeys",
    "$schema",
  ].map((field) => field.toLowerCase()),
);

/**
 * Top-level fields in a raw manifest that this manager does not read, or
 * null when the manifest is not plain JSON (SMAPI also accepts comments and
 * trailing commas) so the field list cannot be read here.
 */
export function unreadManifestFields(raw: string): string[] | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return null;
  }
  return Object.keys(parsed)
    .filter((key) => !READ_FIELDS.has(key.toLowerCase()))
    .sort();
}
