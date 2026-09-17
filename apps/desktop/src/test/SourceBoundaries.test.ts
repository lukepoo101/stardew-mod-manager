import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const sourceRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

// Generated DTOs are owned by Rust and the test folder is test-only code.
const SKIPPED_DIRECTORIES = new Set(["generated", "test"]);

function productionSources(directory: string): string[] {
  const entries = readdirSync(directory);
  const sources: string[] = [];
  for (const entry of entries) {
    const fullPath = path.join(directory, entry);
    if (statSync(fullPath).isDirectory()) {
      if (!SKIPPED_DIRECTORIES.has(entry)) {
        sources.push(...productionSources(fullPath));
      }
      continue;
    }
    if (/\.(test|spec)\./.test(entry) || !/\.tsx?$/.test(entry)) {
      continue;
    }
    sources.push(fullPath);
  }
  return sources;
}

/**
 * Error rendering must go through the structured helpers. Stringifying a
 * rejected Tauri value produces "[object Object]" at best and leaks backend
 * prose at worst.
 */
const STRINGIFIED_ERROR_PATTERNS: { pattern: RegExp; reason: string }[] = [
  {
    pattern: /\bString\(\s*(error|err|e)\s*\)/,
    reason: "use errorSummary(error) instead of String(error)",
  },
  {
    pattern: /\$\{\s*(error|err|e)\s*\}/,
    reason: "use errorSummary(error) instead of interpolating the raw error",
  },
];

describe("frontend error boundary guardrails", () => {
  it("never stringifies an API failure in production feature code", () => {
    const files = productionSources(sourceRoot);
    expect(files.length).toBeGreaterThan(0);

    const violations: string[] = [];
    for (const file of files) {
      const relative = path.relative(sourceRoot, file).replace(/\\/g, "/");
      const lines = readFileSync(file, "utf8").split(/\r?\n/);
      lines.forEach((line, index) => {
        if (line.trim().startsWith("*") || line.trim().startsWith("//")) {
          return;
        }
        for (const { pattern, reason } of STRINGIFIED_ERROR_PATTERNS) {
          if (pattern.test(line)) {
            violations.push(
              `${relative}:${index + 1}: ${reason} -> ${line.trim()}`,
            );
          }
        }
      });
    }

    expect(violations).toEqual([]);
  });

  it("routes every Tauri invoke through the shared wrapper", () => {
    const files = productionSources(sourceRoot);
    const violations: string[] = [];
    for (const file of files) {
      const relative = path.relative(sourceRoot, file).replace(/\\/g, "/");
      if (relative === "shared/api/invoke.ts") {
        continue;
      }
      const source = readFileSync(file, "utf8");
      if (/from "@tauri-apps\/api\/core"/.test(source)) {
        violations.push(
          `${relative}: import the Tauri core API only inside shared/api/invoke.ts`,
        );
      }
    }
    expect(violations).toEqual([]);
  });
});
