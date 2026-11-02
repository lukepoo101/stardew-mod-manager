/**
 * Deterministic local redaction shared by every support output (clipboard
 * summary, bundle, copied log). It only ever produces new strings; the original
 * data is never mutated and redacted originals are never logged.
 *
 * Redaction is best effort. It cannot recognise every piece of personal
 * information, so callers must surface `residualWarnings` for review rather than
 * presenting the output as proven safe.
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
}

// Windows user profile roots: C:\Users\name\..., c:/Users/name/... A name
// can contain spaces ("C:\Users\Jane Doe\AppData"), so when a separator
// follows, everything up to it is the name; otherwise it ends at a space.
const WINDOWS_HOME =
  /\b[A-Za-z]:[\\/]+Users[\\/]+(?:[^\\/"'<>|:*?\r\n]+(?=[\\/])|[^\\/\s"'<>|:*?]+)/gi;
// POSIX home roots: /home/name, /Users/name, and the root user's home.
const POSIX_HOME = /(?<![\w.])\/(?:home|Users)\/[^/\s"'<>|:*?]+/g;
const ROOT_HOME = /(?<![\w.])\/root(?=[/\s"'<>]|$)/g;

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
  /\b((?:host|computer|machine)[ _-]?name\s*[:=]\s*)[^\s,;"']+/gi;
const WINDOWS_DEFAULT_HOST = /\b(?:DESKTOP|LAPTOP)-[A-Z0-9]{5,}\b/g;

// Profile folders that are not a person's name.
const SHARED_PROFILES = new Set([
  "public",
  "default",
  "default user",
  "all users",
  "shared",
  "guest",
]);

// Query parameters that carry signatures or credentials in signed URLs.
const SENSITIVE_QUERY =
  /([?&](?:key|token|signature|sig|expires|x-amz-signature|x-amz-credential|x-amz-security-token|access_token|apikey|api_key|md5)=)[^&\s"'<>]+/gi;

function count(text: string, pattern: RegExp): number {
  return text.match(pattern)?.length ?? 0;
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
} {
  let total = 0;
  const names: string[] = [];
  const replace = (input: string, pattern: RegExp) =>
    input.replace(pattern, (match) => {
      total += 1;
      const name = match.split(/[\\/]+/).pop();
      if (name) names.push(name);
      return "~";
    });
  let out = replace(text, WINDOWS_HOME);
  out = replace(out, POSIX_HOME);
  out = replace(out, ROOT_HOME);
  return { text: out, count: total, names };
}

/** The account names in the home folders of these paths. */
export function accountNamesIn(paths: readonly string[]): string[] {
  return paths.flatMap((path) => redactHomePaths(path).names);
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Every way a name is written that should be hidden: the whole name and, for
 * a name with spaces ("Jane Doe"), each part. Short or shared names are left
 * alone, because hiding them would garble ordinary words.
 */
function nameVariants(names: readonly string[]): string[] {
  const variants = new Set<string>();
  for (const raw of names) {
    const name = raw.trim();
    if (name.length < 3 || SHARED_PROFILES.has(name.toLowerCase())) continue;
    if (name === "root") continue;
    variants.add(name);
    for (const part of name.split(/\s+/)) {
      if (part.length >= 3) variants.add(part);
    }
  }
  // Longest first, so "Jane Doe" goes before "Jane".
  return [...variants].sort((a, b) => b.length - a.length);
}

/**
 * Redacts home paths, secrets and personal details. `knownNames` are account
 * names learned elsewhere (such as the manager's own data folder), hidden
 * wherever they appear, as are the names found in home paths in the text.
 */
export function redactText(
  input: string,
  knownNames: readonly string[] = [],
): RedactionResult {
  const home = redactHomePaths(input);
  let text = home.text;
  let secrets = 0;
  let personal = 0;

  for (const name of nameVariants([...knownNames, ...home.names])) {
    const pattern = new RegExp(
      `(?<![A-Za-z0-9])${escapeRegExp(name)}(?![A-Za-z0-9])`,
      "gi",
    );
    personal += count(text, pattern);
    text = text.replace(pattern, REDACTED_USER_MARKER);
  }
  for (const pattern of STEAM_IDS) {
    personal += count(text, pattern);
    text = text.replace(pattern, REDACTED_ID_MARKER);
  }
  personal += count(text, HOST_LABELLED);
  text = text.replace(HOST_LABELLED, `$1${REDACTED_HOST_MARKER}`);
  personal += count(text, WINDOWS_DEFAULT_HOST);
  text = text.replace(WINDOWS_DEFAULT_HOST, REDACTED_HOST_MARKER);

  for (const pattern of SECRET_PATTERNS) {
    secrets += count(text, pattern);
    text = text.replace(pattern, REDACTED_SECRET_MARKER);
  }
  secrets += count(text, SENSITIVE_QUERY);
  text = text.replace(SENSITIVE_QUERY, `$1${REDACTED_SECRET_MARKER}`);

  return { text, replacements: { paths: home.count, secrets, personal } };
}

/** Enough of a string to find it again, without repeating all of it. */
function excerpt(value: string): string {
  return value.length <= 12 ? value : `${value.slice(0, 6)}…${value.slice(-4)}`;
}

/**
 * Re-scans already redacted output for patterns that redaction should have
 * removed, plus content it cannot judge. The result is a list of review
 * warnings, never a claim that the text is safe.
 */
export function residualWarnings(text: string): string[] {
  const warnings: string[] = [];
  if (new RegExp(WINDOWS_HOME.source, "i").test(text)) {
    warnings.push("A Windows user profile path is still present.");
  }
  if (new RegExp(POSIX_HOME.source).test(text)) {
    warnings.push("A home directory path is still present.");
  }
  if (
    SECRET_PATTERNS.some((pattern) =>
      new RegExp(pattern.source, pattern.flags.replace("g", "")).test(text),
    )
  ) {
    warnings.push("A secret-like value is still present.");
  }
  const opaque = text.match(/[A-Za-z0-9+/]{40,}={0,2}/);
  if (opaque) {
    warnings.push(
      `A long opaque string (${excerpt(opaque[0])}) was found and could be a credential; review it.`,
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
