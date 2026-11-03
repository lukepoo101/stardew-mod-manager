// Small Python-compatibility helpers for the native smoke test.
//
// The smoke test used to be Python, and CI, the docs and anyone reading a
// failed run's artifacts rely on what it wrote: result.json as Python's
// json.dumps(indent=2) produced it, messages that quote strings the way
// Python's repr() does, and text files with the platform's line endings. These
// helpers keep those bytes the same, so the port changes the runtime and
// nothing that a consumer of the output can see.

import { writeFileSync } from "node:fs";

export const IS_WINDOWS = process.platform === "win32";

/** A number that Python held as a float, so it serialises as `15.0`, not `15`. */
export class PyFloat {
  constructor(readonly value: number) {}

  toString(): string {
    return pyFloatRepr(this.value);
  }
}

export function pyFloatRepr(value: number): string {
  if (Number.isInteger(value) && Math.abs(value) < 1e16) {
    return `${value}.0`;
  }
  return String(value);
}

/** Python's `f'{value:g}'` for the small, finite values this script formats. */
export function pyG(value: number | PyFloat): string {
  const number = value instanceof PyFloat ? value.value : value;
  return String(Number(number.toPrecision(6)));
}

function isPrintable(char: string): boolean {
  if (char === " ") return true;
  return !/[\p{C}\p{Z}]/u.test(char);
}

/** Python's `repr()` of a `str`. */
export function pyRepr(text: string): string {
  const quote = text.includes("'") && !text.includes('"') ? '"' : "'";
  let out = quote;
  for (const char of text) {
    if (char === quote || char === "\\") out += `\\${char}`;
    else if (char === "\n") out += "\\n";
    else if (char === "\r") out += "\\r";
    else if (char === "\t") out += "\\t";
    else if (isPrintable(char)) out += char;
    else {
      const code = char.codePointAt(0) ?? 0;
      if (code < 0x100) out += `\\x${code.toString(16).padStart(2, "0")}`;
      else if (code < 0x10000)
        out += `\\u${code.toString(16).padStart(4, "0")}`;
      else out += `\\U${code.toString(16).padStart(8, "0")}`;
    }
  }
  return out + quote;
}

function jsonString(text: string): string {
  // json.dumps with ensure_ascii=True: everything outside ' '..'~' is escaped.
  let out = '"';
  for (let i = 0; i < text.length; i++) {
    const char = text[i] as string;
    const code = text.charCodeAt(i);
    if (char === '"') out += '\\"';
    else if (char === "\\") out += "\\\\";
    else if (char === "\n") out += "\\n";
    else if (char === "\r") out += "\\r";
    else if (char === "\t") out += "\\t";
    else if (char === "\b") out += "\\b";
    else if (char === "\f") out += "\\f";
    else if (code >= 0x20 && code <= 0x7e) out += char;
    else out += `\\u${code.toString(16).padStart(4, "0")}`;
  }
  return `${out}"`;
}

/** Python's `json.dumps(value, indent=2)` for the values a result holds. */
export function pyJsonDumps(value: unknown, indent = 2, level = 0): string {
  if (value === null || value === undefined) return "null";
  if (value instanceof PyFloat) return pyFloatRepr(value.value);
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return String(value);
  if (typeof value === "string") return jsonString(value);
  const inner = " ".repeat(indent * (level + 1));
  const outer = " ".repeat(indent * level);
  if (Array.isArray(value)) {
    if (value.length === 0) return "[]";
    const items = value.map(
      (item) => inner + pyJsonDumps(item, indent, level + 1),
    );
    return `[\n${items.join(",\n")}\n${outer}]`;
  }
  const entries = Object.entries(value as Record<string, unknown>);
  if (entries.length === 0) return "{}";
  const items = entries.map(
    ([key, item]) =>
      `${inner}${jsonString(key)}: ${pyJsonDumps(item, indent, level + 1)}`,
  );
  return `{\n${items.join(",\n")}\n${outer}}`;
}

// Every line boundary str.splitlines() recognises.
// biome-ignore lint/suspicious/noControlCharactersInRegex: \x1c-\x1e are line boundaries in Python
const LINE_BREAKS = /\r\n|[\n\r\v\f\x1c\x1d\x1e\x85\u2028\u2029]/;

/** Python's `str.splitlines()`. */
export function pySplitlines(text: string): string[] {
  const lines = text.split(LINE_BREAKS);
  if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/**
 * Python's text-mode `write_text`: `\n` becomes the platform line separator.
 *
 * The encoding is UTF-8 everywhere. Python used the locale encoding, which is
 * cp1252 on a Windows runner and raised on text it could not encode; that is
 * the one deliberate difference.
 */
export function writeText(path: string, text: string): void {
  writeFileSync(path, IS_WINDOWS ? text.replaceAll("\n", "\r\n") : text);
}

/** Python's `print(text, flush=True)`. */
export function print(text: string): void {
  const line = `${text}\n`;
  process.stdout.write(IS_WINDOWS ? line.replaceAll("\n", "\r\n") : line);
}

/** Python's `time.monotonic()`, in seconds. */
export function monotonic(): number {
  return performance.now() / 1000;
}

/** Python's `time.sleep(seconds)`. */
export function sleep(seconds: number): Promise<void> {
  return Bun.sleep(seconds * 1000);
}

/**
 * Python's `os.environ` as a plain dict.
 *
 * On Windows `os.environ` upper-cases every name, which matters here: the
 * diagnostics filter names by upper-case prefix and print them as they are.
 */
export function environ(): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [name, value] of Object.entries(process.env)) {
    if (value === undefined) continue;
    result[IS_WINDOWS ? name.toUpperCase() : name] = value;
  }
  return result;
}

/** Python's `sorted(dict)`: code-point order of the keys. */
export function sortedKeys(record: Record<string, string>): string[] {
  return Object.keys(record).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
}

// Exceptions, named after the Python ones so a failed run's last line reads
// the same as it did (`AssertionError: Missing 'Mods'; page: ...`).

export class AssertionError extends Error {
  override name = "AssertionError";
}

export class KeyError extends Error {
  override name = "KeyError";
}

export class NameError extends Error {
  override name = "NameError";
}

/** urllib's URLError: the endpoint could not be reached at all. */
export class URLError extends Error {
  override name = "URLError";
  readonly pyModule = "urllib.error";
}

/** A request that did not complete within its timeout. */
export class TimeoutError extends Error {
  override name = "TimeoutError";
}

/**
 * Prints an uncaught error the way Python's traceback ends: the chain, then
 * `Name: message` as the last line. Returns the exit code Python used (1).
 */
export function reportUncaught(error: unknown): number {
  const chain: unknown[] = [];
  for (
    let current: unknown = error;
    current !== undefined && chain.length < 10;
  ) {
    chain.unshift(current);
    current = current instanceof Error ? current.cause : undefined;
  }
  const lines: string[] = [];
  chain.forEach((item, index) => {
    if (index > 0) {
      lines.push(
        "",
        "During handling of the above exception, another exception occurred:",
        "",
      );
    }
    lines.push("Traceback (most recent call last):");
    const stack = item instanceof Error ? (item.stack ?? "") : "";
    for (const frame of stack
      .split("\n")
      .filter((line) => /^\s+at /.test(line))) {
      lines.push(`  ${frame.trim()}`);
    }
    lines.push(describe(item));
  });
  process.stderr.write(`${lines.join("\n")}\n`);
  return 1;
}

function describe(error: unknown): string {
  if (error instanceof Error) {
    // Python qualifies an exception class with its module unless it is a
    // builtin, e.g. scripts.native_smoke.common.DriverError.
    const module = (error as { pyModule?: string }).pyModule;
    const name = module ? `${module}.${error.name}` : error.name;
    return error.message === "" ? name : `${name}: ${error.message}`;
  }
  return String(error);
}
