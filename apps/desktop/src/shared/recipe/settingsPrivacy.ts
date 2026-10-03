/**
 * What a shared settings file might reveal, field by field, and taking it
 * out before sharing. A best-effort scan for common shapes (keys, tokens,
 * passwords, paths from this computer, email addresses); it cannot prove a
 * file is safe. Values are never returned, only where they are.
 */

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

const SECRET_WORDS = [
  "apikey",
  "token",
  "password",
  "passwd",
  "secret",
  "auth",
  "cookie",
  "session",
];
/** Never shared, whatever the curator chooses. */
const BLOCKED_WORDS = ["apikey", "token", "password", "passwd", "secret"];

export type FlagKind = "secret" | "path" | "email";

export interface Flag {
  path: string;
  kind: FlagKind;
  blocked: boolean;
}

const normalise = (key: string) => key.toLowerCase().replace(/[-_ ]/g, "");

function textKind(text: string): FlagKind | null {
  const lower = text.toLowerCase();
  if (
    lower.includes("/home/") ||
    lower.includes("/users/") ||
    lower.includes("c:\\users\\") ||
    lower.includes("c:/users/")
  )
    return "path";
  if (
    text
      .split(/\s+/)
      .some((w) => w.includes("@") && w.includes(".") && !w.startsWith("@"))
  )
    return "email";
  return null;
}

function walk(value: Json, path: string, out: Flag[]) {
  if (Array.isArray(value)) {
    for (const [i, item] of value.entries()) walk(item, `${path}[${i}]`, out);
  } else if (value && typeof value === "object") {
    for (const [key, inner] of Object.entries(value)) {
      const here = path ? `${path}.${key}` : key;
      const k = normalise(key);
      if (
        typeof inner === "string" &&
        inner.trim() &&
        SECRET_WORDS.some((w) => k.includes(w))
      ) {
        out.push({
          path: here,
          kind: "secret",
          blocked: BLOCKED_WORDS.some((w) => k.includes(w)),
        });
        continue;
      }
      walk(inner, here, out);
    }
  } else if (typeof value === "string") {
    const kind = textKind(value);
    if (kind) out.push({ path, kind, blocked: false });
  }
}

/** Field-level findings; `null` when the file is not JSON. */
export function scanSettings(content: string): Flag[] | null {
  let parsed: Json;
  try {
    parsed = JSON.parse(content);
  } catch {
    return null;
  }
  const out: Flag[] = [];
  walk(parsed, "", out);
  return out;
}

function remove(value: Json, paths: ReadonlySet<string>, path: string): Json {
  if (Array.isArray(value))
    return value.map((item, i) => {
      const here = `${path}[${i}]`;
      return paths.has(here) ? "" : remove(item, paths, here);
    });
  if (value && typeof value === "object") {
    const out: { [key: string]: Json } = {};
    for (const [key, inner] of Object.entries(value)) {
      const here = path ? `${path}.${key}` : key;
      if (paths.has(here)) continue;
      out[key] = remove(inner, paths, here);
    }
    return out;
  }
  return value;
}

export interface Redaction {
  content: string;
  /** Fields taken out because they are never shared. */
  blocked: string[];
  /** Fields taken out because the curator chose to leave flagged fields out. */
  removed: string[];
  /** What a fresh scan of the result still finds. */
  remaining: Flag[];
  /** False when the file is not JSON, so fields could not be checked. */
  checked: boolean;
}

/**
 * Takes out blocked fields always, and other flagged fields unless
 * `keepFlagged`, then scans the result again.
 */
export function redactSettings(
  content: string,
  keepFlagged: boolean,
): Redaction {
  const flags = scanSettings(content);
  if (!flags)
    return { content, blocked: [], removed: [], remaining: [], checked: false };
  const blocked = flags.filter((f) => f.blocked).map((f) => f.path);
  const removed = keepFlagged
    ? []
    : flags.filter((f) => !f.blocked).map((f) => f.path);
  if (blocked.length + removed.length === 0)
    return { content, blocked, removed, remaining: flags, checked: true };
  const out = `${JSON.stringify(
    remove(JSON.parse(content), new Set([...blocked, ...removed]), ""),
    null,
    2,
  )}\n`;
  return {
    content: out,
    blocked,
    removed,
    remaining: scanSettings(out) ?? [],
    checked: true,
  };
}

/** SHA-256 of settings text with CRLF read as LF, as the backend computes it. */
export async function settingsSha256(content: string): Promise<string> {
  const bytes = new TextEncoder().encode(content.replace(/\r\n/g, "\n"));
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)]
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

/** Which fields changed between two versions of a settings file, by path. */
export function changedFields(before: string, after: string): string[] | null {
  let a: Json;
  let b: Json;
  try {
    a = JSON.parse(before);
    b = JSON.parse(after);
  } catch {
    return null;
  }
  const flat = (value: Json, path: string, out: Map<string, string>) => {
    if (value && typeof value === "object" && !Array.isArray(value)) {
      for (const [k, v] of Object.entries(value))
        flat(v, path ? `${path}.${k}` : k, out);
    } else out.set(path, JSON.stringify(value));
    return out;
  };
  const x = flat(a, "", new Map());
  const y = flat(b, "", new Map());
  return [...new Set([...x.keys(), ...y.keys()])]
    .filter((k) => x.get(k) !== y.get(k))
    .sort();
}

export interface SettingsChange {
  mod: string;
  path: string;
  change: "added" | "removed" | "changed";
  /** For a changed JSON file, which fields; `null` when not comparable. */
  fields: string[] | null;
}

/**
 * How shared settings differ between two revisions, file by file and field
 * by field. Values are never included.
 */
export function settingsChanges(
  before: readonly {
    unique_id: string;
    name: string;
    settings?: { path: string; sha256: string; content: string }[];
  }[],
  after: readonly {
    unique_id: string;
    name: string;
    settings?: { path: string; sha256: string; content: string }[];
  }[],
): SettingsChange[] {
  const index = (list: typeof before) =>
    new Map(
      list.flatMap((c) =>
        (c.settings ?? []).map(
          (s) => [`${c.unique_id.toLowerCase()}/${s.path}`, { c, s }] as const,
        ),
      ),
    );
  const a = index(before);
  const b = index(after);
  const out: SettingsChange[] = [];
  for (const [key, { c, s }] of b) {
    const was = a.get(key);
    if (!was)
      out.push({ mod: c.name, path: s.path, change: "added", fields: null });
    else if (was.s.sha256 !== s.sha256)
      out.push({
        mod: c.name,
        path: s.path,
        change: "changed",
        fields: changedFields(was.s.content, s.content),
      });
  }
  for (const [key, { c, s }] of a)
    if (!b.has(key))
      out.push({ mod: c.name, path: s.path, change: "removed", fields: null });
  return out.sort((x, y) => x.mod.localeCompare(y.mod));
}
