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
    const wrapperPath = "shared/api/invoke.ts";
    const files = productionSources(sourceRoot);
    const violations: string[] = [];
    for (const file of files) {
      const relative = path.relative(sourceRoot, file).replace(/\\/g, "/");
      if (relative === wrapperPath) {
        continue;
      }
      // Static and dynamic imports are both rejected: the guard must not depend
      // on the import syntax a caller happens to pick.
      if (readFileSync(file, "utf8").includes("@tauri-apps/api/core")) {
        violations.push(
          `${relative}: the Tauri core API may only be reached through ${wrapperPath}`,
        );
      }
    }
    expect(violations).toEqual([]);

    // Keep the guard honest: if the wrapper is renamed or deleted, this test
    // fails instead of silently scanning for a module nobody imports.
    expect(readFileSync(path.join(sourceRoot, wrapperPath), "utf8")).toContain(
      "@tauri-apps/api/core",
    );
  });
});
