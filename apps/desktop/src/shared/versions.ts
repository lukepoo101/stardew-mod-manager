/**
 * Numeric-aware comparison of dotted mod versions ("1.10" is after "1.2").
 * Missing parts count as zero, so "1.0" equals "1.0.0"; parts that are not
 * numbers compare as text. Returns -1, 0 or 1.
 */
export function compareVersions(a: string, b: string): number {
  const left = a.split(/[.+-]/);
  const right = b.split(/[.+-]/);
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const x = left[index] ?? "0";
    const y = right[index] ?? "0";
    const nx = Number(x);
    const ny = Number(y);
    const numeric =
      x !== "" && y !== "" && !Number.isNaN(nx) && !Number.isNaN(ny);
    const order = numeric ? nx - ny : x.localeCompare(y);
    if (order !== 0) return order < 0 ? -1 : 1;
  }
  return 0;
}

/** A version as the backend's `SmapiVersion` reads it. */
interface SmapiVersion {
  parts: [number, number, number, number];
  prerelease: string | null;
}

const U32_MAX = 4294967295;

/** Rust's `u32::from_str`: ASCII digits with an optional leading "+". */
const parseU32 = (text: string): number | null => {
  if (!/^\+?[0-9]+$/.test(text)) return null;
  const value = Number(text.replace(/^\+/, ""));
  return value <= U32_MAX ? value : null;
};

/**
 * The same parse as `manager_core::version::SmapiVersion::parse`: one to
 * four numeric parts, then an optional "-prerelease". Anything else (a "v"
 * prefix, "1.x") is unparseable and returns null.
 */
export function parseSmapiVersion(text: string): SmapiVersion | null {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  const dash = trimmed.indexOf("-");
  const core = dash === -1 ? trimmed : trimmed.slice(0, dash);
  const prerelease = dash === -1 ? null : trimmed.slice(dash + 1);
  const pieces = core.split(".");
  if (pieces.length > 4) return null;
  const parts: [number, number, number, number] = [0, 0, 0, 0];
  for (const [index, piece] of pieces.entries()) {
    const value = parseU32(piece);
    if (value === null) return null;
    parts[index] = value;
  }
  return { parts, prerelease };
}

const bytes = (text: string) => new TextEncoder().encode(text);

/** Rust's `String` ordering: byte-wise over UTF-8. */
function compareBytes(a: string, b: string): number {
  const x = bytes(a);
  const y = bytes(b);
  for (let i = 0; i < Math.min(x.length, y.length); i += 1) {
    if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
  }
  return Math.sign(x.length - y.length);
}

/** `SmapiVersion`'s ordering: a prerelease sorts below its release. */
function compareSmapi(a: SmapiVersion, b: SmapiVersion): number {
  for (let i = 0; i < 4; i += 1) {
    if (a.parts[i] !== b.parts[i]) return a.parts[i] < b.parts[i] ? -1 : 1;
  }
  if (a.prerelease === null && b.prerelease === null) return 0;
  if (a.prerelease === null) return 1;
  if (b.prerelease === null) return -1;
  return compareBytes(a.prerelease, b.prerelease);
}

/**
 * Whether `have` meets a MinimumVersion, decided exactly as the backend's
 * install check decides it: when either version cannot be parsed the
 * requirement counts as met.
 */
export function meetsMinimumVersion(have: string, minimum: string): boolean {
  const a = parseSmapiVersion(have);
  const b = parseSmapiVersion(minimum);
  if (!a || !b) return true;
  return compareSmapi(a, b) >= 0;
}
