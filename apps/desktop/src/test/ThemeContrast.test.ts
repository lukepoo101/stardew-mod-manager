import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = path.dirname(fileURLToPath(import.meta.url));
const css = readFileSync(path.join(here, "../styles/tokens.css"), "utf8");

/** The `--name: #hex` tokens of one block (":root" or ".dark"). */
function tokens(selector: string): Record<string, string> {
  const start = css.indexOf(`${selector} {`);
  const body = css.slice(start, css.indexOf("}", start));
  const out: Record<string, string> = {};
  for (const match of body.matchAll(/--([a-z-]+):\s*(#[0-9a-f]{6})/gi)) {
    out[match[1]] = match[2];
  }
  return out;
}

function luminance(hex: string): number {
  const channel = (i: number) => {
    const c = Number.parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// Text pairs need 4.5:1 (WCAG AA); focus indicators need 3:1 against the
// surfaces they sit on.
const TEXT_PAIRS: Array<[string, string]> = [
  ["fg-primary", "bg-primary"],
  ["fg-primary", "bg-surface"],
  ["fg-primary", "bg-elevated"],
  ["fg-muted", "bg-primary"],
  ["fg-muted", "bg-surface"],
  ["danger", "bg-surface"],
  ["danger", "danger-surface"],
  ["warning", "bg-surface"],
  ["warning", "warning-surface"],
  ["success", "bg-surface"],
  ["success", "success-surface"],
  ["accent-fg", "accent-primary"],
  ["accent-fg", "accent-hover"],
  ["danger-fg", "danger"],
  ["accent-fg", "success"],
  ["accent-primary", "bg-surface"],
];
const FOCUS_PAIRS: Array<[string, string]> = [
  ["border-focus", "bg-primary"],
  ["border-focus", "bg-surface"],
];

describe.each([
  ["light", ":root"],
  ["dark", ".dark"],
])("%s theme contrast", (_name, selector) => {
  const t = { ...tokens(":root"), ...tokens(selector) };
  it.each(TEXT_PAIRS)("%s on %s is readable", (fg, bg) => {
    expect(contrast(t[fg], t[bg])).toBeGreaterThanOrEqual(4.5);
  });
  it.each(FOCUS_PAIRS)("%s on %s is visible", (fg, bg) => {
    expect(contrast(t[fg], t[bg])).toBeGreaterThanOrEqual(3);
  });
});
