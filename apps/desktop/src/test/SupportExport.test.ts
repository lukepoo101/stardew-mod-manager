import { describe, expect, it } from "vitest";
import type {
  DiagnosticsDto,
  ModListItemDto,
  OperationDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import { redactText, residualWarnings } from "@/shared/support/redact";
import {
  buildSupportPlan,
  planWarnings,
  renderSupportBundle,
  renderSupportSummary,
} from "@/shared/support/export";
import { buildInventory, serializeInventory } from "@/shared/support/inventory";

// Synthetic canaries: shaped like credentials, but not real ones.
const CANARY_TOKEN = "canarytoken1234567890";
const CANARY_GH = "ghp_CANARY0123456789abcdefghij";

describe("redaction", () => {
  it("replaces home prefixes and keeps the relative structure", () => {
    const { text, replacements } = redactText(
      "Loaded /home/luke/.config/StardewValley/ErrorLogs/SMAPI-latest.txt and C:\\Users\\Luke\\AppData\\Roaming\\x and /Users/amy/Library/y and /root/z",
    );
    expect(text).toContain(
      "~/.config/StardewValley/ErrorLogs/SMAPI-latest.txt",
    );
    expect(text).toContain("~\\AppData\\Roaming\\x");
    expect(text).toContain("~/Library/y");
    expect(text).toContain("~/z");
    expect(text).not.toMatch(/luke|amy|Luke/);
    expect(replacements.paths).toBe(4);
  });

  it("redacts tokens, authorization headers, credential URLs and signed URLs", () => {
    const input = [
      `Authorization: Bearer ${CANARY_TOKEN}`,
      `apikey=${CANARY_TOKEN}`,
      `password: "${CANARY_TOKEN}"`,
      `https://user:${CANARY_TOKEN}@example.com/x`,
      `${CANARY_GH}`,
      `https://cdn.example.com/f.zip?expires=99&signature=${CANARY_TOKEN}&name=ok`,
    ].join("\n");
    const { text, replacements } = redactText(input);
    expect(text).not.toContain(CANARY_TOKEN);
    expect(text).not.toContain(CANARY_GH);
    expect(text).toContain("name=ok");
    expect(replacements.secrets).toBeGreaterThanOrEqual(6);
    expect(residualWarnings(text)).toEqual([]);
  });

  it("does not mangle ordinary text that only resembles a secret", () => {
    const input =
      "Loaded token cache for the tokenizer mod; homepage /homepage/x";
    expect(redactText(input).text).toBe(input);
  });

  it("warns about content it cannot judge instead of calling it safe", () => {
    const opaque = "A".repeat(48);
    expect(residualWarnings(`blob ${opaque}`).join()).toMatch(/opaque/);
    expect(
      residualWarnings("contact me at someone@example.com").join(),
    ).toMatch(/email/);
  });
});

const overview = {
  profile: { name: "Default", revision: 3, mod_count: 2 },
  game: { operating_system: "Linux", storefront: "Steam" },
  mod_count: 2,
  smapi_status: {
    is_installed: true,
    observed_version: "4.1.10",
    tested_version: "4.1.10",
    is_compatible: true,
  },
} as unknown as ProfileOverviewDto;

const mods = [
  {
    unique_id: "Z.Mod",
    name: "Z",
    author: "a",
    version: "1.0.0",
    enabled: true,
    installed_reason: "explicit",
    artifact_hash: "abc",
  },
  {
    unique_id: "A.Mod",
    name: "A",
    author: "a",
    version: "2.0.0",
    enabled: false,
    installed_reason: "explicit",
    artifact_hash: "",
  },
] as unknown as ModListItemDto[];

const report = {
  host_operating_system: "Linux",
  findings: [],
  raw_log: `line\n[ERROR] failed at /home/luke/Mods/X token=${CANARY_TOKEN}`,
} as unknown as DiagnosticsDto;

const generatedAt = "2026-09-29T00:00:00.000Z";

describe("support export", () => {
  const plan = buildSupportPlan({
    report,
    overview,
    mods,
    managerVersion: "1.2.3",
    generatedAt,
  });

  it("redacts every section and keeps clipboard and bundle in step", () => {
    const summary = renderSupportSummary(plan);
    const bundle = renderSupportBundle(plan);
    for (const output of [summary, bundle]) {
      expect(output).not.toContain(CANARY_TOKEN);
      expect(output).not.toContain("/home/luke");
      expect(output).toContain("~/Mods/X");
    }
    expect(
      JSON.parse(bundle).sections.map((s: { text: string }) => s.text),
    ).toEqual(plan.sections.map((s) => s.text));
  });

  it("lists mods by exact id and version in a stable order", () => {
    const summary = renderSupportSummary(plan);
    expect(summary.indexOf("A.Mod 2.0.0 disabled")).toBeLessThan(
      summary.indexOf("Z.Mod 1.0.0 enabled"),
    );
  });

  it("does not present an empty findings list as proof of health", () => {
    expect(renderSupportSummary(plan)).toMatch(/not proof/);
  });

  it("marks missing evidence as unavailable and warns about it", () => {
    const empty = buildSupportPlan({
      report: undefined,
      overview: undefined,
      mods: undefined,
      generatedAt,
    });
    const summary = renderSupportSummary(empty);
    expect(summary).toMatch(/Environment \(unavailable\)/);
    expect(summary).toMatch(/Installed mods \(unavailable\)/);
    expect(planWarnings(empty).join()).toMatch(/unavailable/);
  });

  it("honours a deselected optional section in both outputs", () => {
    const deselected = new Set(["log" as const]);
    expect(renderSupportSummary(plan, deselected)).not.toMatch(/SMAPI log/);
    expect(
      JSON.parse(renderSupportBundle(plan, deselected)).sections.map(
        (s: { id: string }) => s.id,
      ),
    ).not.toContain("log");
  });

  it("marks findings the user dismissed, without hiding them", () => {
    const plan = buildSupportPlan({
      report: {
        ...report,
        findings: [
          {
            fingerprint: "f1",
            code: "SMAPI_MISSING",
            severity: "warning",
            summary: "SMAPI is required",
          },
        ],
      } as unknown as DiagnosticsDto,
      overview,
      mods,
      acknowledged: new Set(["f1"]),
      generatedAt,
    });
    const findings = plan.sections.find((s) => s.id === "findings");
    expect(findings?.text).toContain("(dismissed by the user; still present)");
  });

  it("only keeps the tail of a long log", () => {
    const long = buildSupportPlan({
      report: {
        ...report,
        raw_log: Array.from({ length: 500 }, (_, i) => `row ${i}`).join("\n"),
      } as DiagnosticsDto,
      overview,
      mods,
      generatedAt,
    });
    const text = renderSupportSummary(long);
    expect(text).toContain("row 499");
    expect(text).not.toContain("row 10\n");
  });

  it("lists only the profile's recent changes, newest first, redacted", () => {
    const op = (id: string, profile: string, created: string, extra = {}) =>
      ({
        id,
        kind: "mod_install",
        state: "succeeded",
        profile_id: profile,
        error_code: null,
        created_at: created,
        ...extra,
      }) as unknown as OperationDto;
    const withHistory = buildSupportPlan({
      report,
      overview: {
        ...overview,
        profile: { ...overview.profile, id: "p1" },
      } as ProfileOverviewDto,
      mods,
      operations: [
        op("old", "p1", "2026-09-01T00:00:00Z"),
        op("other", "p2", "2026-09-03T00:00:00Z"),
        op("failed", "p1", "2026-09-02T00:00:00Z", {
          state: "failed",
          error_code: "DISK_FULL",
        }),
      ],
      generatedAt,
    });
    const history = withHistory.sections.find((s) => s.id === "history");
    expect(history?.text).toBe(
      "2026-09-02T00:00:00Z mod_install failed (DISK_FULL)\n2026-09-01T00:00:00Z mod_install succeeded",
    );
    expect(history?.unavailable).toBe(false);
    const without = buildSupportPlan({ report, overview, mods, generatedAt });
    expect(without.sections.find((s) => s.id === "history")?.unavailable).toBe(
      true,
    );
  });
});

describe("inventory export", () => {
  it("is versioned, sorted, deterministic and path free", () => {
    const options = { generatedAt, managerVersion: "1.2.3" };
    const first = serializeInventory(buildInventory(overview, mods, options));
    const reversed = serializeInventory(
      buildInventory(overview, [...mods].reverse(), options),
    );
    expect(first).toBe(reversed);
    const parsed = JSON.parse(first);
    expect(parsed.schema).toBe("stardew-mod-manager.inventory");
    expect(parsed.schema_version).toBe(1);
    expect(
      parsed.components.map((c: { unique_id: string }) => c.unique_id),
    ).toEqual(["A.Mod", "Z.Mod"]);
    expect(first).not.toMatch(/deployment|\/home|canonical_root/);
  });

  it("represents an unknown source explicitly", () => {
    const parsed = JSON.parse(
      serializeInventory(buildInventory(overview, mods, { generatedAt })),
    );
    expect(parsed.components[0].source.kind).toBe("unknown");
    expect(parsed.components[1].source.kind).toBe("local");
  });

  it("lists every requirement problem and says files were not checked", () => {
    const withProblems = {
      ...overview,
      health_summary: {
        status: "error",
        error_count: 2,
        warning_count: 0,
        info_count: 0,
        findings: [
          { code: "DEPENDENCY_DISABLED", affected_entities: ["A.Mod"] },
          { code: "DEPENDENCY_TOO_OLD", affected_entities: ["A.Mod"] },
        ],
      },
    } as unknown as ProfileOverviewDto;
    const parsed = JSON.parse(
      serializeInventory(buildInventory(withProblems, mods, { generatedAt })),
    );
    const a = parsed.components.find(
      (c: { unique_id: string }) => c.unique_id === "A.Mod",
    );
    expect(a.requirement_problems).toEqual(["disabled", "too_old"]);
    expect(a.files_checked).toBe(false);
    expect(a.folder_missing).toBe(false);
  });
});
