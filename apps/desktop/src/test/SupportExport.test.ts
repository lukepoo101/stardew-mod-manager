import { describe, expect, it } from "vitest";
import type {
  DiagnosticsDto,
  ModListItemDto,
  OperationDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import {
  accountNamesIn,
  redactText,
  residualWarnings,
} from "@/shared/support/redact";
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

  it("hides the account name, real name, host and Steam id from the exploratory test", () => {
    // The lines the exploratory test put in a SMAPI log (finding F27).
    const input = [
      "/home/luke/Documents/My Farm Notes/secret.txt",
      "C:\\Users\\Luke Hann\\AppData\\Roaming\\StardewValley\\Saves",
      "Logged in as Luke Hann (luke) on host luke-desktop",
      "steam=76561198012345678 [U:1:52079950] STEAM_0:1:26039975",
      "Mods go here: /tmp/claude-1000/-home-luke-repos-x/Mods",
      "hostname: gaming-rig, machine name = DESKTOP-7Q2X9ZB",
      `token=${CANARY_GH}`,
    ].join("\n");
    const { text, replacements } = redactText(input);
    expect(text).toContain("~/Documents/My Farm Notes/secret.txt");
    expect(text).toContain("~\\AppData\\Roaming\\StardewValley\\Saves");
    expect(text).not.toMatch(/luke|hann/i);
    expect(text).not.toContain("76561198012345678");
    expect(text).not.toContain("52079950");
    expect(text).not.toContain("26039975");
    expect(text).not.toContain("gaming-rig");
    expect(text).not.toContain("DESKTOP-7Q2X9ZB");
    expect(text).not.toContain(CANARY_GH);
    expect(text).toContain(
      "Logged in as <redacted-user> (<redacted-user>) on host <redacted-host>",
    );
    expect(text).toContain("-home-<redacted-user>-repos-x/Mods");
    expect(replacements.personal).toBeGreaterThanOrEqual(8);
  });

  it("hides an account name learned from the manager's own folders", () => {
    const names = accountNamesIn([
      "D:\\Users\\jdoe\\AppData\\Roaming\\stardew-mod-manager",
    ]);
    expect(names).toEqual(["jdoe"]);
    const { text } = redactText("Library on /mnt/games/jdoe/Stardew Valley", {
      knownNames: names,
    });
    expect(text).toBe("Library on /mnt/games/<redacted-user>/Stardew Valley");
  });

  it("never turns ordinary words, characters or mod names into redactions", () => {
    const cases: [string, string][] = [
      // Windows' common "User" account.
      [
        "C:\\Users\\User\\AppData\nThe user cancelled. Check User settings. (set by the user in Settings)",
        "~\\AppData\nThe user cancelled. Check User settings. (set by the user in Settings)",
      ],
      // An account named like a word.
      [
        "C:\\Users\\Will Turner\\Saves\nSMAPI will skip it. Will Turner? Logged in as Will Turner",
        "~\\Saves\nSMAPI will skip it. <redacted-user>? Logged in as <redacted-user>",
      ],
      // An account named after a Stardew character.
      [
        "/home/sam/.config\n[Sam's Dialogue Expansion] Loaded Sam's dialogue for Sam and Sebastian. user: sam",
        "~/.config\n[Sam's Dialogue Expansion] Loaded Sam's dialogue for Sam and Sebastian. user: <redacted-user>",
      ],
      // The Steam Deck's account.
      [
        "/home/deck/.steam\nRunning on a Steam Deck",
        "~/.steam\nRunning on a Steam Deck",
      ],
      // A mod author who is the user: UniqueIDs stay intact.
      [
        "/home/luke/x\nLoaded luke.MyMod 1.0.0 and Pathoschild.luke",
        "~/x\nLoaded luke.MyMod 1.0.0 and Pathoschild.luke",
      ],
      // A message carrying on after a bare profile path.
      [
        "Game folder C:\\Users\\Bob has 3 mods and 2 content packs/folders skipped",
        "Game folder ~ has 3 mods and 2 content packs/folders skipped",
      ],
      [
        "Backed up to C:\\Users\\Bob, 5 files / 2 MB",
        "Backed up to ~, 5 files / 2 MB",
      ],
    ];
    for (const [input, expected] of cases) {
      expect(redactText(input).text).toBe(expected);
    }
    // Ordinary words left in prose are not reported as leaks either.
    expect(
      residualWarnings("Running on a Steam Deck", ["deck"]).join(),
    ).not.toMatch(/account name/);
  });

  it("treats only letters as part of the name inside paths", () => {
    const names = ["sam"];
    const cases: [string, string][] = [
      ["/mnt/sam_games/x", "/mnt/<redacted-user>_games/x"],
      ["/mnt/sam.backup/x", "/mnt/<redacted-user>.backup/x"],
      ["/mnt/sam2/x", "/mnt/<redacted-user>2/x"],
      // Inside a longer word, it stays.
      ["/mnt/samantha/Games", "/mnt/samantha/Games"],
      ["Mods/[CP] Samurai/x", "Mods/[CP] Samurai/x"],
    ];
    for (const [input, expected] of cases) {
      const { text } = redactText(input, { knownNames: names });
      expect(text).toBe(expected);
    }
    // The warning check uses the same rule.
    expect(residualWarnings("/mnt/sam_games/x", names).join()).toMatch(
      /"sam" still appears in a path/,
    );
    expect(residualWarnings("/mnt/samantha/Games", names)).toEqual([]);
  });

  it("hides a folder named after one word of a profile name, comparing in NFC", () => {
    const names = accountNamesIn([
      "C:\\Users\\Jean-Luc Picard\\AppData\\Roaming",
    ]);
    expect(names).toEqual(["Jean-Luc Picard"]);
    expect(
      redactText("Library at D:\\Jean-Luc\\Games\\x", { knownNames: names })
        .text,
    ).toBe("Library at D:\\<redacted-user>\\Games\\x");
    // "José" written with a combining accent still matches.
    const decomposed = "Jose\u0301";
    expect(
      redactText(`D:\\${decomposed}\\Games`, { knownNames: ["José Müller"] })
        .text,
    ).toBe("D:\\<redacted-user>\\Games");
  });

  it("hides an ordinary-word account name inside paths and addresses, not in prose", () => {
    const { text } = redactText(
      [
        "/home/sam/.config",
        "Library: /mnt/games/sam-games/SteamLibrary",
        "sam@sam-pc:~$ ls",
        "Mods go here: /tmp/x/-home-sam-repos-x/Mods",
        "[Sam's Dialogue Expansion] Loaded Sam's dialogue for Sam and Sebastian",
      ].join("\n"),
    );
    expect(text).toBe(
      [
        "~/.config",
        "Library: /mnt/games/<redacted-user>-games/SteamLibrary",
        "<redacted-user>@<redacted-user>-pc:~$ ls",
        "Mods go here: /tmp/x/-home-<redacted-user>-repos-x/Mods",
        "[Sam's Dialogue Expansion] Loaded Sam's dialogue for Sam and Sebastian",
      ].join("\n"),
    );
    // The same for a "user" account; prose about the user is untouched.
    expect(
      redactText(
        "C:\\Users\\User\\x\n/tmp/x/-home-user-repos-x/Mods\nThe user cancelled.",
      ).text,
    ).toBe(
      "~\\x\n/tmp/x/-home-<redacted-user>-repos-x/Mods\nThe user cancelled.",
    );
    // Left in a path, it is pointed out.
    expect(residualWarnings("/mnt/games/sam-games", ["sam"]).join()).toMatch(
      /"sam" still appears in a path/,
    );
  });

  it("takes a whole Windows profile folder name and warns when it may be cut", () => {
    const whole: [string, string][] = [
      [
        "C:\\Users\\Sean O'Brien\\AppData\\Roaming\\StardewValley",
        "~\\AppData\\Roaming\\StardewValley",
      ],
      [
        "C:\\Users\\Mary Jane Van Dyke\\AppData\\Roaming",
        "~\\AppData\\Roaming",
      ],
      ["C:\\Users\\Mary Jane Van Dyke\\Saved Games\\x", "~\\Saved Games\\x"],
    ];
    for (const [input, expected] of whole) {
      const result = redactText(input);
      expect(result.text).toBe(expected);
      expect(result.review).toEqual([]);
    }
    // A message after a bare profile path: the first word is taken, and the
    // export asks for a look.
    const cut = redactText(
      "Game folder C:\\Users\\Bob has 3 mods and 2 content packs/folders skipped",
    );
    expect(cut.text).toBe(
      "Game folder ~ has 3 mods and 2 content packs/folders skipped",
    );
    expect(cut.review.join()).toMatch(/may continue after/);
    const plan = buildSupportPlan({
      report: {
        ...report,
        raw_log: "Saves in C:\\Users\\Anne Marie Louise Smith/x",
      },
      overview,
      mods,
      generatedAt,
    });
    expect(planWarnings(plan).join()).toMatch(/may continue after/);
  });

  it("finds homes on other Linux layouts, WSL and in other scripts", () => {
    const cases: [string, string][] = [
      ["/var/home/luke/.local/share/Steam", "~/.local/share/Steam"],
      [
        "/run/media/luke/Games/SteamLibrary",
        "/run/media/<redacted-user>/Games/SteamLibrary",
      ],
      ["/media/luke/Games", "/media/<redacted-user>/Games"],
      ["/mnt/c/Users/luke/AppData", "~/AppData"],
      [
        "C:\\Users\\田中\\x\nLogged in as 田中",
        "~\\x\nLogged in as <redacted-user>",
      ],
      ["Saves are in C:\\Users\\Luke Hann", "Saves are in ~"],
    ];
    for (const [input, expected] of cases) {
      expect(redactText(input).text).toBe(expected);
    }
  });

  it("points out an account name left in another case, and not ordinary paths", () => {
    const { text } = redactText("/home/luke/x\nLuke said hi");
    expect(text).toBe("~/x\nLuke said hi");
    expect(residualWarnings(text, ["luke"]).join()).toMatch(
      /account name "luke" still appears/,
    );
    expect(
      residualWarnings(
        "~/.local/share/stardew-mod-manager/setups/profile/Mods/ContentPatcher/assets/data",
      ),
    ).toEqual([]);
  });

  it("leaves short and shared profile names alone", () => {
    const input = "C:\\Users\\Public\\x and /home/al/y; Public al deck";
    expect(redactText(input).text).toBe("~\\x and ~/y; Public al deck");
  });

  it("warns about content it cannot judge instead of calling it safe", () => {
    const opaque = "A1".repeat(24);
    expect(residualWarnings(`blob ${opaque}`).join()).toMatch(
      /opaque string \(A1A1A1…A1A1\)/,
    );
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
    expect(renderSupportSummary(plan, deselected)).not.toMatch(/## SMAPI log/);
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

describe("support export additions", () => {
  it("summarises log errors, names unchecked areas and lists a manifest", () => {
    const base = {
      log_summary: {
        smapi_version: null,
        game_version: null,
        loaded_mod_count: null,
        skipped_mods: [
          {
            name: "Needy",
            version: "1.0",
            reason: "it needs mod Z.Lib",
            missing_dependencies: ["Z.Lib"],
            line: 3,
          },
        ],
        update_notices: [],
        sources: [],
        total_lines: 3,
        errors: [
          {
            line: 2,
            source: "A",
            kind: "exception",
            message: "NullReferenceException",
          },
        ],
      },
      raw_log: "line\nline\nline",
      findings: [],
    } as unknown as DiagnosticsDto;
    const plan = buildSupportPlan({
      report: undefined,
      overview: undefined,
      mods: [],
      generatedAt: "2026-10-02T00:00:00Z",
      logReport: base,
      logSource: "the saved log of the session at 10:00",
    });
    const summary = renderSupportSummary(plan);
    expect(summary).toMatch(/Skipped: Needy 1.0 because it needs mod Z.Lib/);
    expect(summary).toMatch(/1 error line\(s\) of kind exception/);
    expect(summary).toMatch(/Not checked by this manager: mod compatibility/);
    expect(summary).toMatch(/from the saved log of the session at 10:00/);
    const bundle = JSON.parse(renderSupportBundle(plan));
    expect(bundle.manifest.map((m: { id: string }) => m.id)).toContain(
      "errors",
    );
    expect(bundle.manifest[0].characters).toBeGreaterThan(0);
  });
});

describe("support export health and game version", () => {
  const finding = (code: string, severity: string, fingerprint = code) => ({
    id: code,
    fingerprint,
    code,
    severity,
    category: "dependency",
    title: `${code} title`,
    summary: `${code} summary`,
    affected_entities: [],
    evidence: [],
    observed_at: "",
  });
  const withHealth = {
    ...overview,
    game: { ...overview.game, canonical_root: "/home/jdoe/games/sdv" },
    smapi_status: {
      ...overview.smapi_status,
      game_version: "1.6.15",
      game_version_set_by_user: true,
    },
    health_summary: {
      findings: [
        finding("DEPENDENCY_DISABLED", "error"),
        finding("GAME_VERSION_OVERRIDDEN", "info"),
      ],
    },
  } as unknown as ProfileOverviewDto;

  it("includes the profile's health errors and the game version the user set", () => {
    const plan = buildSupportPlan({
      report: { ...report, findings: [finding("LOG_MOD_ERRORS", "warning")] },
      overview: withHealth,
      mods,
      generatedAt,
    });
    const text = renderSupportSummary(plan);
    expect(text).toMatch(
      /Stardew Valley 1\.6\.15 \(set by the user in Settings, not detected\)/,
    );
    expect(text).toMatch(/\[error\] DEPENDENCY_DISABLED/);
    expect(text).toMatch(/\[info\] GAME_VERSION_OVERRIDDEN/);
    expect(text).toMatch(/From the SMAPI log:\n\[warning\] LOG_MOD_ERRORS/);
    expect(text).not.toMatch(/No findings were reported/);
    // The account name in the game folder is hidden wherever it appears.
    expect(text).not.toContain("jdoe");
  });

  it("keeps UniqueIDs and the manager's own wording when the account name is a word in them", () => {
    const plan = buildSupportPlan({
      report: {
        ...report,
        app_data_dir: "C:\\Users\\User\\AppData\\Roaming\\smm",
        raw_log: "[luke.Tools] loaded from /home/luke/Mods",
      },
      overview: withHealth,
      mods: [
        { ...mods[0], unique_id: "luke.Tools", name: "Luke's Tools" },
      ] as unknown as ModListItemDto[],
      generatedAt,
    });
    const text = renderSupportSummary(plan);
    expect(text).toContain("luke.Tools 1.0.0 enabled");
    expect(text).toContain("[luke.Tools] loaded from ~/Mods");
    expect(text).toContain("(set by the user in Settings, not detected)");
  });

  it("does not call a log it does not have error free", () => {
    const plan = buildSupportPlan({
      report,
      overview: withHealth,
      mods,
      generatedAt,
      logReport: { ...report, raw_log: "" },
      logSource: "the saved log of an old session",
    });
    const errors = plan.sections.find((s) => s.id === "errors");
    expect(errors?.unavailable).toBe(true);
    expect(errors?.text).not.toMatch(/no errors/);
  });
});
