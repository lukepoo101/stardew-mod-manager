/**
 * Deterministic local redaction shared by every support output (clipboard
 * summary, bundle, copied log). It only ever produces new strings; the original
 * data is never mutated and redacted originals are never logged.
 *
 * Redaction is best effort. It cannot recognise every piece of personal
 * information, so callers must surface `residualWarnings` for review rather than
 * presenting the output as proven safe.
 *
 * Account names are learned from home folders in the text (and from the
 * manager's own folders, passed in). Inside paths and `name@host` tokens a
 * name is hidden wherever it is a whole word, in any case, whatever the
 * word. Elsewhere it is hidden where a phrase says it is an account ("logged
 * in as", "user:") and, unless it is also an ordinary word (see `ORDINARY`)
 * or a word from the profile's own mods, wherever it appears whole, written
 * exactly as learned. It is never hidden inside a longer word or a UniqueID.
 */

export const REDACTED_PATH_MARKER = "<redacted-path>";
export const REDACTED_SECRET_MARKER = "<redacted-secret>";
export const REDACTED_USER_MARKER = "<redacted-user>";
export const REDACTED_ID_MARKER = "<redacted-id>";
export const REDACTED_HOST_MARKER = "<redacted-host>";

export interface RedactionResult {
  text: string;
  /** How many substitutions were made, by kind. Never contains the originals. */
  replacements: { paths: number; secrets: number; personal: number };
  /** Things redaction was unsure about, for the review warnings. */
  review: string[];
}

export const TRUNCATED_PROFILE_WARNING =
  'A Windows profile folder name may continue after a "~"; check that none of it is left.';

export interface RedactOptions {
  /** Account names learned elsewhere, such as the manager's data folder. */
  knownNames?: readonly string[];
  /**
   * Words never hidden outside paths and account phrases, such as the
   * profile's mod names and UniqueID parts.
   */
  protectedWords?: readonly string[];
  /**
   * Whether to hide account names outside paths at all. Off for the list of
   * installed mods, whose exact UniqueIDs are the point.
   */
  names?: boolean;
}

// Windows user profile roots: C:\Users\name\..., c:/Users/name/... The name
// segment is everything up to the next separator; whether all of it is the
// name is decided in `windowsHome` (a name can contain a space).
// An apostrophe counts only between letters ("Sean O'Brien").
const WINDOWS_HOME_START =
  /\b([A-Za-z]:[\\/]+Users[\\/]+)((?:[^\\/"'<>|:*?\r\n,;()]|(?<=\p{L})'(?=\p{L}))+)/giu;
// Folders every Windows profile has: a segment followed by one of these is a
// whole profile folder name, however many words it has.
const PROFILE_SUBFOLDER =
  /^[\\/](?:AppData|Documents|Desktop|Downloads|OneDrive[^\\/]*|Saved Games|Music|Pictures|Videos|Favorites|Contacts|Links|Searches|3D Objects|NTUSER\.DAT|\.[\w-]+)(?=[\\/\s"']|$)/i;
// For residual warnings: a Windows profile path with any name.
const WINDOWS_HOME = /\b[A-Za-z]:[\\/]+Users[\\/]+[^\\/\s"'<>|:*?]+/i;
// POSIX homes: /home/name (also /var/home on Fedora Atomic and Bazzite),
// macOS /Users/name, WSL /mnt/c/Users/name and the root user's home.
const POSIX_HOMES = [
  /(?<![\w.])(?:\/var)?\/home\/([^/\s"'<>|:*?]+)/g,
  /(?<![\w.])\/mnt\/[A-Za-z]\/Users\/([^/\s"'<>|:*?]+)/gi,
  /(?<![\w.])\/Volumes\/[^/]+\/Users\/([^/\s"'<>|:*?]+)/g,
  /(?<![\w.])\/Users\/([^/\s"'<>|:*?]+)/g,
];
const POSIX_HOME = /(?<![\w.])(?:\/var)?\/(?:home|Users)\/[^/\s"'<>|:*?]+/;
const ROOT_HOME = /(?<![\w.])\/root(?=[/\s"'<>]|$)/g;
// Removable drives are mounted under the user's name: the name is hidden and
// the rest of the path kept.
const MEDIA_MOUNTS = /(?<![\w.])(\/(?:run\/)?media\/)([^/\s"'<>|:*?]+)/g;

const SECRET_PATTERNS: RegExp[] = [
  // Authorization headers and bearer tokens.
  /\b(?:authorization\s*[:=]\s*)(?:bearer|basic|token)\s+[A-Za-z0-9._~+/=-]+/gi,
  /\bbearer\s+[A-Za-z0-9._~+/=-]{8,}/gi,
  // key=value / key: value pairs for well known credential names.
  /\b(?:api[_-]?key|apikey|access[_-]?token|refresh[_-]?token|auth[_-]?token|token|secret|password|passwd|client[_-]?secret)\s*["']?\s*[:=]\s*["']?[^\s"'&,;]{4,}/gi,
  // Credentials embedded in a URL: scheme://user:pass@host
  /\b[a-z][a-z0-9+.-]*:\/\/[^\s/:@]+:[^\s/@]+@/gi,
  // GitHub and similar prefixed tokens.
  /\b(?:ghp|gho|ghu|ghs|ghr|github_pat)_[A-Za-z0-9_]{16,}/g,
  // JSON Web Tokens.
  /\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g,
];

// Steam account ids: SteamID64 (7656119...), SteamID3 ([U:1:n]) and the
// legacy STEAM_0:1:n form.
const STEAM_IDS = [
  /\b7656119\d{10}\b/g,
  /\[U:1:\d+\]/g,
  /\bSTEAM_[0-5]:[01]:\d+\b/g,
];

// A computer's name where a log labels it, and Windows' default names.
const HOST_LABELLED =
  /\b((?:host|computer|machine)[ _-]?name\s*[:=]\s*|on host\s+)[^\s,;"']+/gi;
const WINDOWS_DEFAULT_HOST = /\b(?:DESKTOP|LAPTOP)-[A-Z0-9]{5,}\b/g;

// Profile folders that are not a person's name.
const SHARED_PROFILES = new Set([
  "public",
  "default",
  "default user",
  "all users",
  "shared",
  "guest",
  "root",
]);

/**
 * Account names that are also ordinary words, or names of Stardew Valley
 * characters: hidden in paths and account phrases only, never in prose, where
 * hiding them would garble messages and mod names.
 */
const ORDINARY = new Set(
  [
    // Generic account names and common words.
    "user users admin administrator owner player gamer deck steam home test",
    "tester dev developer mod mods game games farm farmer modder local",
    "will may june april august grace hope joy faith max mark bill art rose",
    "sky summer winter spring fall autumn star stardew valley",
    // Stardew Valley characters.
    "abigail alex birdie caroline clint demetrius dwarf elliott emily evelyn",
    "george gil governor grandpa gunther gus haley harvey jas jodi kent",
    "krobus leah leo lewis linus marlon marnie maru morris pam penny pierre",
    "qi robin sam sandy sebastian shane vincent willy wizard",
  ]
    .join(" ")
    .split(" "),
);

// Query parameters that carry signatures or credentials in signed URLs.
const SENSITIVE_QUERY =
  /([?&](?:key|token|signature|sig|expires|x-amz-signature|x-amz-credential|x-amz-security-token|access_token|apikey|api_key|md5)=)[^&\s"'<>]+/gi;

// A run of text that is a path (it has a separator) or a user@host address.
const PATH_OR_HOST_TOKEN =
  /[^\s"'<>|]*[\\/][^\s"'<>|]*|[\w.+-]+@[\w.-]+(?::[^\s"'<>|]*)?/g;

// Letters, digits and "_" make a name part of a longer word; a "." between
// word characters makes it part of a UniqueID.
const BEFORE = "(?<![\\p{L}\\p{N}_]|[\\p{L}\\p{N}_]\\.)";
const AFTER = "(?![\\p{L}\\p{N}_]|\\.[\\p{L}\\p{N}_])";

function count(text: string, pattern: RegExp): number {
  return text.match(pattern)?.length ?? 0;
}

/**
 * Decides how much of a Windows profile segment is the name. A name can
 * contain a space ("C:\Users\Jane Doe\AppData"), but a message can also
 * carry on after a bare profile path ("C:\Users\Bob has 3 mods/2 packs").
 * The whole segment is the name when it is short: up to three words before
 * a separator, or two at the end of the line. Otherwise it is the first word.
 */
function windowsName(segment: string, rest: string): string {
  const whole = segment.trimEnd();
  if (whole.length <= 64 && PROFILE_SUBFOLDER.test(rest)) return whole;
  const words = segment.split(" ").length;
  const next = rest.charAt(0);
  const atSeparator = next === "\\" || next === "/";
  const atEnd = next === "" || next === "\n" || next === "\r" || next === '"';
  if (
    segment.length <= 40 &&
    ((atSeparator && words <= 3) || (atEnd && words <= 2))
  ) {
    return whole;
  }
  return segment.split(" ")[0];
}

/**
 * Replaces home-directory prefixes with a stable placeholder that keeps the
 * relative structure: `/home/luke/.config/x` becomes `~/.config/x`. The
 * account names found in them are returned, to redact elsewhere too.
 */
function redactHomePaths(text: string): {
  text: string;
  count: number;
  names: string[];
  /** Profile folder names that may have been cut short. */
  truncated: number;
} {
  let total = 0;
  let truncated = 0;
  const names: string[] = [];
  let out = text.replace(
    WINDOWS_HOME_START,
    (
      _match,
      _prefix: string,
      segment: string,
      offset: number,
      whole: string,
    ) => {
      const rest = whole.slice(offset + _match.length);
      const name = windowsName(segment, rest);
      if (name.length < segment.trimEnd().length) truncated += 1;
      total += 1;
      names.push(name);
      return `~${segment.slice(name.length)}`;
    },
  );
  for (const pattern of POSIX_HOMES) {
    out = out.replace(pattern, (_match, name: string) => {
      total += 1;
      names.push(name);
      return "~";
    });
  }
  out = out.replace(MEDIA_MOUNTS, (_match, prefix: string, name: string) => {
    total += 1;
    names.push(name);
    return `${prefix}${REDACTED_USER_MARKER}`;
  });
  total += count(out, ROOT_HOME);
  out = out.replace(ROOT_HOME, "~");
  return { text: out, count: total, names, truncated };
}

/** The account names in the home folders of these paths. */
export function accountNamesIn(paths: readonly string[]): string[] {
  return paths.flatMap((path) => redactHomePaths(path).names);
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Names worth hiding: long enough not to be a fragment, and not shared. */
function usableNames(names: readonly string[]): string[] {
  const usable = new Set<string>();
  for (const raw of names) {
    const name = raw.trim();
    // Two characters are enough for a name in another script ("田中").
    const minimum = [...name].some((ch) => ch.charCodeAt(0) > 127) ? 2 : 3;
    if (name.length < minimum) continue;
    if (SHARED_PROFILES.has(name.toLowerCase())) continue;
    usable.add(name);
  }
  // Longest first, so "Jane Doe" goes before a shorter name inside it.
  return [...usable].sort((a, b) => b.length - a.length);
}

function isOrdinary(
  name: string,
  protectedWords: ReadonlySet<string>,
): boolean {
  const lower = name.toLowerCase();
  return ORDINARY.has(lower) || protectedWords.has(lower);
}

/**
 * Redacts home paths, secrets and personal details. See the module comment
 * for where account names are hidden. A list of names may be passed instead
 * of options, for `knownNames`.
 */
export function redactText(
  input: string,
  options: RedactOptions | readonly string[] = {},
): RedactionResult {
  const opts: RedactOptions = Array.isArray(options)
    ? { knownNames: options as readonly string[] }
    : (options as RedactOptions);
  const protectedWords = new Set(
    (opts.protectedWords ?? []).map((w) => w.toLowerCase()),
  );
  const home = redactHomePaths(input);
  let text = home.text;
  let secrets = 0;
  let personal = 0;
  const hide = (pattern: RegExp, replacement: string) => {
    personal += count(text, pattern);
    text = text.replace(pattern, replacement);
  };

  for (const name of usableNames([...(opts.knownNames ?? []), ...home.names])) {
    const escaped = escapeRegExp(name);
    // Inside paths and name@host tokens (not prose), as a whole word in any
    // case, even when it is an ordinary word: "/mnt/games/sam-games",
    // "sam@sam-pc".
    const word = new RegExp(`${BEFORE}${escaped}${AFTER}`, "giu");
    text = text.replace(PATH_OR_HOST_TOKEN, (token) => {
      personal += count(token, word);
      return token.replace(word, REDACTED_USER_MARKER);
    });
    // A whole path component, which can contain a space ("D:\Luke Hann\x").
    hide(
      new RegExp(
        `(?<=[\\\\/])${escaped}(?=[\\\\/]|$|[\\s"'<>|:*?,;)])`,
        "gimu",
      ),
      REDACTED_USER_MARKER,
    );
    // Said to be an account.
    hide(
      new RegExp(
        `\\b((?:user(?:name)?|login|account|owner|logged in as|signed in as|running as)\\s*[:=]?\\s*["']?)${escaped}${AFTER}`,
        "giu",
      ),
      `$1${REDACTED_USER_MARKER}`,
    );
    hide(
      new RegExp(`${BEFORE}${escaped}(?=@[\\w-])`, "giu"),
      REDACTED_USER_MARKER,
    );
    // Anywhere else, written exactly as learned, unless it is also a word.
    if (opts.names !== false && !isOrdinary(name, protectedWords)) {
      hide(
        new RegExp(`${BEFORE}${escaped}${AFTER}`, "gu"),
        REDACTED_USER_MARKER,
      );
    }
  }
  for (const pattern of STEAM_IDS) hide(pattern, REDACTED_ID_MARKER);
  hide(HOST_LABELLED, `$1${REDACTED_HOST_MARKER}`);
  hide(WINDOWS_DEFAULT_HOST, REDACTED_HOST_MARKER);

  for (const pattern of SECRET_PATTERNS) {
    secrets += count(text, pattern);
    text = text.replace(pattern, REDACTED_SECRET_MARKER);
  }
  secrets += count(text, SENSITIVE_QUERY);
  text = text.replace(SENSITIVE_QUERY, `$1${REDACTED_SECRET_MARKER}`);

  return {
    text,
    replacements: { paths: home.count, secrets, personal },
    review: home.truncated > 0 ? [TRUNCATED_PROFILE_WARNING] : [],
  };
}

/** Enough of a string to find it again, without repeating all of it. */
function excerpt(value: string): string {
  return value.length <= 12 ? value : `${value.slice(0, 6)}…${value.slice(-4)}`;
}

/**
 * Re-scans already redacted output for patterns that redaction should have
 * removed, plus content it cannot judge. The result is a list of review
 * warnings, never a claim that the text is safe. `names` are the account
 * names redaction was given, so any left in place (as an ordinary word, or
 * written differently) are pointed out.
 */
export function residualWarnings(
  text: string,
  names: readonly string[] = [],
): string[] {
  const warnings: string[] = [];
  if (WINDOWS_HOME.test(text)) {
    warnings.push("A Windows user profile path is still present.");
  }
  if (POSIX_HOME.test(text)) {
    warnings.push("A home directory path is still present.");
  }
  if (
    SECRET_PATTERNS.some((pattern) =>
      new RegExp(pattern.source, pattern.flags.replace("g", "")).test(text),
    )
  ) {
    warnings.push("A secret-like value is still present.");
  }
  for (const name of usableNames(names)) {
    const word = new RegExp(`${BEFORE}${escapeRegExp(name)}${AFTER}`, "iu");
    // Ordinary words are left in prose on purpose; warning about them in
    // every export would teach people to tick through the warnings. In a
    // path or address they should never be left.
    if (ORDINARY.has(name.toLowerCase())) {
      if ((text.match(PATH_OR_HOST_TOKEN) ?? []).some((t) => word.test(t))) {
        warnings.push(
          `Your account name "${name}" still appears in a path or address; review it.`,
        );
      }
      continue;
    }
    if (word.test(text)) {
      warnings.push(
        `Your account name "${name}" still appears, as an ordinary word or written differently; review it.`,
      );
    }
  }
  // Paths are not credentials: "/" is left out, and a credential mixes
  // letters and digits.
  const opaque = (text.match(/[A-Za-z0-9+_-]{40,}={0,2}/g) ?? []).find(
    (candidate) => /\d/.test(candidate) && /[A-Za-z]/.test(candidate),
  );
  if (opaque) {
    warnings.push(
      `A long opaque string (${excerpt(opaque)}) was found and could be a credential; review it.`,
    );
  }
  const email = text.match(/\b[\w.+-]+@[\w-]+\.[a-z]{2,}\b/i);
  if (email) {
    warnings.push(`An email address (${email[0]}) was found in the export.`);
  }
  return warnings;
}

export function mergeCounts(
  a: RedactionResult["replacements"],
  b: RedactionResult["replacements"],
): RedactionResult["replacements"] {
  return {
    paths: a.paths + b.paths,
    secrets: a.secrets + b.secrets,
    personal: a.personal + b.personal,
  };
}
