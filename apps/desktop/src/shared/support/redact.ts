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

export interface RedactionResult {
  text: string;
  /** How many substitutions were made, by kind. Never contains the originals. */
  replacements: { paths: number; secrets: number };
}

// Windows user profile roots: C:\Users\name\..., c:/Users/name/...
const WINDOWS_HOME = /\b[A-Za-z]:[\\/]+Users[\\/]+[^\\/\s"'<>|:*?]+/gi;
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

// Query parameters that carry signatures or credentials in signed URLs.
const SENSITIVE_QUERY =
  /([?&](?:key|token|signature|sig|expires|x-amz-signature|x-amz-credential|x-amz-security-token|access_token|apikey|api_key|md5)=)[^&\s"'<>]+/gi;

function count(text: string, pattern: RegExp): number {
  return text.match(pattern)?.length ?? 0;
}

/**
 * Replaces home-directory prefixes with a stable placeholder that keeps the
 * relative structure: `/home/luke/.config/x` becomes `~/.config/x`.
 */
function redactHomePaths(text: string): { text: string; count: number } {
  let total = 0;
  const replace = (input: string, pattern: RegExp) => {
    total += count(input, pattern);
    return input.replace(pattern, "~");
  };
  let out = replace(text, WINDOWS_HOME);
  out = replace(out, POSIX_HOME);
  out = replace(out, ROOT_HOME);
  return { text: out, count: total };
}

export function redactText(input: string): RedactionResult {
  const home = redactHomePaths(input);
  let text = home.text;
  let secrets = 0;

  for (const pattern of SECRET_PATTERNS) {
    secrets += count(text, pattern);
    text = text.replace(pattern, REDACTED_SECRET_MARKER);
  }
  secrets += count(text, SENSITIVE_QUERY);
  text = text.replace(SENSITIVE_QUERY, `$1${REDACTED_SECRET_MARKER}`);

  return { text, replacements: { paths: home.count, secrets } };
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
  if (/[A-Za-z0-9+/]{40,}={0,2}/.test(text)) {
    warnings.push(
      "A long opaque string was found and could be a credential; review it.",
    );
  }
  if (/\b[\w.+-]+@[\w-]+\.[a-z]{2,}\b/i.test(text)) {
    warnings.push("An email address was found in the export.");
  }
  return warnings;
}

export function mergeCounts(
  a: RedactionResult["replacements"],
  b: RedactionResult["replacements"],
): RedactionResult["replacements"] {
  return { paths: a.paths + b.paths, secrets: a.secrets + b.secrets };
}
