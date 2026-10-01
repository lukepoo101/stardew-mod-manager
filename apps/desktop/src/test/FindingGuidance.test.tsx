import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { guidanceFor, knownFindingCodes } from "@/shared/health/guidance";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../../..",
);

/** Every finding code the backend can emit, read from its source. */
function emittedCodes(): string[] {
  const files = ["health.rs", "diagnostics.rs"].map((name) =>
    readFileSync(
      path.join(repoRoot, "crates/manager-app/src/services", name),
      "utf8",
    ),
  );
  const codes = new Set<string>();
  for (const text of files) {
    for (const match of text.matchAll(/code: "([A-Z_]+)"\.to_string\(\)/g)) {
      codes.add(match[1]);
    }
    for (const match of text.matchAll(/\("((?:RUNTIME|MOD)_[A-Z_]+)"/g)) {
      codes.add(match[1]);
      // Codes that also have an "unable to assess" form.
      if (match[1].startsWith("MOD_") && text.includes(`{code}_UNASSESSED`))
        codes.add(`${match[1]}_UNASSESSED`);
    }
    // Codes chosen in a match arm alongside their severity.
    for (const match of text.matchAll(
      /"([A-Z][A-Z_]+)",\s*"(?:error|warning|info)"/g,
    )) {
      codes.add(match[1]);
    }
  }
  return [...codes].sort();
}

describe("finding guidance", () => {
  it("covers every finding code the backend emits", () => {
    const emitted = emittedCodes();
    expect(emitted.length).toBeGreaterThan(5);
    const missing = emitted.filter(
      (code) => !knownFindingCodes().includes(code),
    );
    expect(missing).toEqual([]);
  });

  it("says to investigate by hand rather than inventing an action", () => {
    const unknown = guidanceFor({ code: "SOMETHING_NEW" });
    expect(unknown.action).toBeNull();
    expect(unknown.impact).toBe("");
  });

  it("marks runtime changes as inference, not a detected failure", () => {
    expect(guidanceFor({ code: "RUNTIME_SMAPI_CHANGED" }).certainty).toBe(
      "inferred",
    );
    expect(guidanceFor({ code: "LOG_MOD_SKIPPED" }).certainty).toBe("observed");
  });

  it("only routes to pages that exist", () => {
    const routes = new Set([
      "/app/overview",
      "/app/mods",
      "/app/profiles",
      "/app/diagnostics",
      "/app/activity",
      "/app/settings",
    ]);
    for (const code of knownFindingCodes()) {
      const action = guidanceFor({ code }).action;
      if (action) expect(routes.has(action.to)).toBe(true);
    }
  });
});
