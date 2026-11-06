/**
 * What a shared settings file might reveal, field by field, and taking it
 * out before sharing. A best-effort scan for common shapes (keys, tokens,
 * passwords, paths from this computer, email addresses); it cannot prove a
 * file is safe. Values are never returned, only where they are.
 *
 * Redaction edits the file's own text: only the fields taken out change, so
 * comments, key order and number formatting stay as the mod wrote them, and
 * the backend (`manager-core::settings_privacy`) writes exactly the same text
 * for the same file. Both follow the same rules and share their test cases.
 */

import { isKeybind } from "./buttons";

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
/**
 * A field is never shared when its name (lower case, without `-`, `_` or
 * spaces) is one of these or ends with one: "ApiToken", "AccountPassword".
 * "ShowSecretNotes", "SecretWoods" and "MaxTokens" are ordinary settings.
 */
const BLOCKED_ENDINGS = [
  "apikey",
  "authkey",
  "accesskey",
  "privatekey",
  "token",
  "password",
  "passwd",
  "secret",
];

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

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

/** A settings file read as a tree of positions in its own text. */
type Node =
  | { kind: "object"; open: number; close: number; members: Member[] }
  | { kind: "array"; items: Item[] }
  | { kind: "text"; text: string }
  | { kind: "number" | "bool" | "null" };
interface Member {
  key: string;
  /** Where its key's opening quote is. */
  start: number;
  /** Just after its value. */
  end: number;
  value: Node;
}
interface Item {
  start: number;
  end: number;
  value: Node;
}

/**
 * The deepest nesting of objects and lists read, as in the backend
 * (`MAX_DEPTH`). Anything deeper is "not JSON" rather than a stack overflow.
 */
export const MAX_DEPTH = 128;

/** Reads JSON as SMAPI accepts it (comments, trailing commas, a byte-order
 * mark), keeping where each value is; `null` if it is not JSON. */
function readTree(text: string): Node | null {
  let i = text.startsWith("\uFEFF") ? 1 : 0;
  let depth = 0;
  const fail = (): never => {
    throw new Error("not JSON");
  };
  const space = () => {
    for (;;) {
      const c = text[i];
      if (c === " " || c === "\t" || c === "\n" || c === "\r") i += 1;
      else if (text.startsWith("//", i)) {
        const n = text.indexOf("\n", i);
        i = n < 0 ? text.length : n;
      } else if (text.startsWith("/*", i)) {
        const n = text.indexOf("*/", i + 2);
        i = n < 0 ? text.length : n + 2;
      } else return;
    }
  };
  const string = (): string => {
    const start = i;
    i += 1;
    for (;;) {
      const c = text[i];
      if (c === undefined) fail();
      if (c === "\\") i += 2;
      else if (c === '"') {
        i += 1;
        break;
      } else i += 1;
    }
    try {
      return JSON.parse(text.slice(start, i)) as string;
    } catch {
      return fail();
    }
  };
  /** One value, refusing to nest deeper than MAX_DEPTH. */
  const value = (): Node => {
    const nests = text[i] === "{" || text[i] === "[";
    if (nests) {
      if (depth >= MAX_DEPTH) fail();
      depth += 1;
    }
    const node = valueHere();
    if (nests) depth -= 1;
    return node;
  };
  const valueHere = (): Node => {
    const c = text[i];
    if (c === undefined) return fail();
    if (c === "{") {
      const open = i;
      i += 1;
      const members: Member[] = [];
      for (;;) {
        space();
        if (text[i] === "}") break;
        if (text[i] !== '"') fail();
        const start = i;
        const key = string();
        space();
        if (text[i] !== ":") fail();
        i += 1;
        space();
        const inner = value();
        members.push({ key, start, end: i, value: inner });
        space();
        if (text[i] === ",") i += 1;
        else if (text[i] === "}") break;
        else fail();
      }
      const close = i;
      i += 1;
      return { kind: "object", open, close, members };
    }
    if (c === "[") {
      i += 1;
      const items: Item[] = [];
      for (;;) {
        space();
        if (text[i] === "]") break;
        const start = i;
        const inner = value();
        items.push({ start, end: i, value: inner });
        space();
        if (text[i] === ",") i += 1;
        else if (text[i] === "]") break;
        else fail();
      }
      i += 1;
      return { kind: "array", items };
    }
    if (c === '"') return { kind: "text", text: string() };
    for (const [word, kind] of [
      ["true", "bool"],
      ["false", "bool"],
      ["null", "null"],
    ] as const) {
      if (text.startsWith(word, i)) {
        i += word.length;
        return { kind };
      }
    }
    const start = i;
    while (i < text.length && /[-+.eE0-9]/.test(text[i])) i += 1;
    if (
      !/^-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?$/.test(
        text.slice(start, i),
      )
    )
      fail();
    return { kind: "number" };
  };
  try {
    space();
    const node = value();
    space();
    return i === text.length ? node : null;
  } catch {
    return null;
  }
}

/**
 * Whether a field named `key` (normalised) holding `value` is never
 * shared. True or false is never a secret, nor is nothing. Under a name
 * ending in "Key" or "Secret", a key binding ("F3", "LeftShift + F3": SMAPI
 * button names, see `buttons.ts`) is a hotkey, not a credential; never
 * under "Password", "Passwd" or "Token" names. Any other text, and any
 * number, is taken out.
 */
function neverShared(key: string, value: Node): boolean {
  if (!BLOCKED_ENDINGS.some((w) => key.endsWith(w))) return false;
  const hotkeyName = key.endsWith("key") || key.endsWith("secret");
  const holds = (node: Node): boolean => {
    switch (node.kind) {
      case "text":
        return node.text.trim() !== "" && !(hotkeyName && isKeybind(node.text));
      case "number":
        return true;
      case "object":
        return node.members.some((m) => holds(m.value));
      case "array":
        return node.items.some((item) => holds(item.value));
      default:
        return false;
    }
  };
  return holds(value);
}

type Edit = [from: number, to: number, replacement: string];

function walkTree(
  text: string,
  node: Node,
  path: string,
  keepFlagged: boolean,
  found: Flag[],
  edits: Edit[],
) {
  if (node.kind === "object") {
    const gone: number[] = [];
    node.members.forEach((member, index) => {
      const here = path ? `${path}.${member.key}` : member.key;
      const k = normalise(member.key);
      if (neverShared(k, member.value)) {
        found.push({ path: here, kind: "secret", blocked: true });
        gone.push(index);
        return;
      }
      const inner = member.value;
      if (
        inner.kind === "text" &&
        inner.text.trim() &&
        !isKeybind(inner.text) &&
        SECRET_WORDS.some((w) => k.includes(w))
      ) {
        found.push({ path: here, kind: "secret", blocked: false });
        if (!keepFlagged) gone.push(index);
        return;
      }
      if (inner.kind === "text") {
        const kind = textKind(inner.text);
        if (kind) {
          found.push({ path: here, kind, blocked: false });
          if (!keepFlagged) gone.push(index);
        }
        return;
      }
      walkTree(text, inner, here, keepFlagged, found, edits);
    });
    if (gone.length === 0) return;
    const members = node.members;
    if (gone.length === members.length) {
      edits.push([node.open + 1, node.close, ""]);
      return;
    }
    // Each run of fields taken out goes with the separator after it, or, at
    // the end, the one before it.
    for (let run = 0; run < gone.length; run += 1) {
      const first = gone[run];
      let last = first;
      while (run + 1 < gone.length && gone[run + 1] === last + 1) {
        run += 1;
        last += 1;
      }
      if (last + 1 < members.length)
        edits.push([members[first].start, members[last + 1].start, ""]);
      else {
        // To the end of the object, so a comment after the field (which may
        // repeat its value) goes with it; the line break before the closing
        // brace stays.
        const tail = text.slice(members[last].end, node.close);
        const n = tail.lastIndexOf("\n");
        const to =
          n >= 0 && tail.slice(n).trim() === ""
            ? members[last].end + n
            : node.close;
        edits.push([members[first - 1].end, to, ""]);
      }
    }
  } else if (node.kind === "array") {
    node.items.forEach((item, index) => {
      const here = `${path}[${index}]`;
      if (item.value.kind === "text") {
        const kind = textKind(item.value.text);
        if (kind) {
          found.push({ path: here, kind, blocked: false });
          if (!keepFlagged) edits.push([item.start, item.end, '""']);
        }
        return;
      }
      walkTree(text, item.value, here, keepFlagged, found, edits);
    });
  }
}

/** Field-level findings; `null` when the file is not JSON. */
export function scanSettings(content: string): Flag[] | null {
  const tree = readTree(content);
  if (!tree) return null;
  const found: Flag[] = [];
  walkTree(content, tree, "", true, found, []);
  return found;
}

export interface Redaction {
  content: string;
  /** Fields taken out because they are never shared. */
  blocked: string[];
  /** Fields taken out because the curator chose to leave flagged fields out. */
  removed: string[];
  /** What a fresh scan of the result still finds. */
  remaining: Flag[];
  /** False when the file is not a JSON object this can read field by field.
   * Then `content` is empty: such a file is never shared. */
  checked: boolean;
}

/**
 * Takes out blocked fields always, and other flagged fields unless
 * `keepFlagged`, then scans the result again. Only those fields change; a
 * file with nothing taken out is returned exactly as it is. A file that is
 * not a settings object this can read is never returned, as in the backend.
 */
export function redactSettings(
  content: string,
  keepFlagged: boolean,
): Redaction {
  const tree = readTree(content);
  if (tree?.kind !== "object")
    return {
      content: "",
      blocked: [],
      removed: [],
      remaining: [],
      checked: false,
    };
  const found: Flag[] = [];
  const edits: Edit[] = [];
  walkTree(content, tree, "", keepFlagged, found, edits);
  const blocked: string[] = [];
  const removed: string[] = [];
  for (const f of found) {
    const list = f.blocked ? blocked : keepFlagged ? null : removed;
    if (list && !list.includes(f.path)) list.push(f.path);
  }
  let out = content;
  for (const [from, to, replacement] of edits.sort((a, b) => b[0] - a[0]))
    out = out.slice(0, from) + replacement + out.slice(to);
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

/**
 * Which fields changed between two versions of a settings file, by path.
 * Settings text can come from someone else's recipe, so it is first read
 * with the depth-limited reader: nesting past MAX_DEPTH is not compared.
 */
export function changedFields(before: string, after: string): string[] | null {
  if (!readTree(before) || !readTree(after)) return null;
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

/** Changed fields split into added, removed and changed; `null` if not JSON. */
export function fieldChanges(
  before: string,
  after: string,
): { added: string[]; removed: string[]; changed: string[] } | null {
  const paths = changedFields(before, after);
  if (!paths) return null;
  const has = (text: string, path: string) => {
    let value: unknown = JSON.parse(text);
    for (const part of path.split(".")) {
      if (!value || typeof value !== "object" || !(part in value)) return false;
      value = (value as Record<string, unknown>)[part];
    }
    return true;
  };
  const out = {
    added: [] as string[],
    removed: [] as string[],
    changed: [] as string[],
  };
  for (const path of paths) {
    const was = has(before, path);
    const is = has(after, path);
    if (!was) out.added.push(path);
    else if (!is) out.removed.push(path);
    else out.changed.push(path);
  }
  return out;
}

export interface SettingsChange {
  mod: string;
  path: string;
  change: "added" | "removed" | "changed";
  /** For a changed JSON file, which fields; `null` when not comparable. */
  fields: { added: string[]; removed: string[]; changed: string[] } | null;
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
        fields: fieldChanges(was.s.content, s.content),
      });
  }
  for (const [key, { c, s }] of a)
    if (!b.has(key))
      out.push({ mod: c.name, path: s.path, change: "removed", fields: null });
  return out.sort((x, y) => x.mod.localeCompare(y.mod));
}
