import { describe, expect, it } from "vitest";
import {
  changedFields,
  MAX_DEPTH,
  redactSettings,
  scanSettings,
  settingsChanges,
  settingsSha256,
} from "@/shared/recipe/settingsPrivacy";

const file = JSON.stringify({
  Speed: 2,
  ApiKey: "abc123",
  Auth: { Session: "xyz" },
  SavePath: "/home/ann/saves",
  Contact: "ann@example.com",
  Friends: ["/home/ann/x", "plain"],
});

describe("settings privacy", () => {
  it("finds fields by path, never returning values", () => {
    const flags = scanSettings(file) ?? [];
    expect(flags).toEqual(
      expect.arrayContaining([
        { path: "ApiKey", kind: "secret", blocked: true },
        { path: "Auth.Session", kind: "secret", blocked: false },
        { path: "SavePath", kind: "path", blocked: false },
        { path: "Contact", kind: "email", blocked: false },
        { path: "Friends[0]", kind: "path", blocked: false },
      ]),
    );
    expect(JSON.stringify(flags)).not.toContain("abc123");
    expect(scanSettings("not json")).toBeNull();
  });

  it("always takes out blocked fields, others unless kept, then rescans", () => {
    const strict = redactSettings(file, false);
    expect(strict.blocked).toEqual(["ApiKey"]);
    expect(strict.removed).toContain("SavePath");
    expect(strict.remaining).toEqual([]);
    expect(JSON.parse(strict.content)).toEqual({
      Speed: 2,
      Auth: {},
      Friends: ["", "plain"],
    });

    const kept = redactSettings(file, true);
    expect(kept.blocked).toEqual(["ApiKey"]);
    expect(kept.removed).toEqual([]);
    expect(kept.remaining.map((f) => f.path)).toContain("SavePath");
    expect(kept.content).not.toContain("abc123");

    const plain = redactSettings('{"Speed":1}', false);
    expect(plain.content).toBe('{"Speed":1}');
    expect(redactSettings("not json", false).checked).toBe(false);
  });

  it("checksums like the backend, ignoring line endings", async () => {
    expect(await settingsSha256("{}")).toBe(
      "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
    );
    expect(await settingsSha256("{\r\n}")).toBe(await settingsSha256("{\n}"));
  });

  it("tells added, removed and changed fields apart", async () => {
    const { fieldChanges } = await import("@/shared/recipe/settingsPrivacy");
    expect(fieldChanges('{"a":1,"b":2}', '{"a":3,"c":4}')).toEqual({
      added: ["c"],
      removed: ["b"],
      changed: ["a"],
    });
  });

  it("names changed fields between revisions, without values", () => {
    expect(
      changedFields('{"a":1,"b":{"c":2}}', '{"a":1,"b":{"c":3},"d":4}'),
    ).toEqual(["b.c", "d"]);
    const before = [
      {
        unique_id: "A",
        name: "A",
        settings: [{ path: "config.json", sha256: "1", content: '{"x":1}' }],
      },
      {
        unique_id: "B",
        name: "B",
        settings: [{ path: "config.json", sha256: "2", content: "{}" }],
      },
    ];
    const after = [
      {
        unique_id: "A",
        name: "A",
        settings: [{ path: "config.json", sha256: "3", content: '{"x":2}' }],
      },
      {
        unique_id: "C",
        name: "C",
        settings: [{ path: "config.json", sha256: "4", content: "{}" }],
      },
    ];
    expect(settingsChanges(before, after)).toEqual([
      {
        mod: "A",
        path: "config.json",
        change: "changed",
        fields: { added: [], removed: [], changed: ["x"] },
      },
      { mod: "B", path: "config.json", change: "removed", fields: null },
      { mod: "C", path: "config.json", change: "added", fields: null },
    ]);
  });
});

/**
 * The backend's cases word for word (`manager-core::settings_privacy`,
 * SHARED_CASES), so the collection flow and bundles write the same bytes.
 * [file, keep flagged, shared text, never shared, left out]
 */
const SHARED_CASES: [string, boolean, string, string[], string[]][] = [
  // Ordinary settings that only contain a secret-like word.
  [
    '{"ShowSecretNotes": true, "Volume": 3}',
    true,
    '{"ShowSecretNotes": true, "Volume": 3}',
    [],
    [],
  ],
  [
    '{"ShowSecretNotes": true, "Volume": 3}',
    false,
    '{"ShowSecretNotes": true, "Volume": 3}',
    [],
    [],
  ],
  [
    '{"SecretWoods": {"Enabled": true}, "Volume": 3}',
    true,
    '{"SecretWoods": {"Enabled": true}, "Volume": 3}',
    [],
    [],
  ],
  [
    '{"SecretWoods": {"Enabled": true}, "Volume": 3}',
    false,
    '{"SecretWoods": {"Enabled": true}, "Volume": 3}',
    [],
    [],
  ],
  [
    '{"MaxTokens": 5, "Volume": 3}',
    true,
    '{"MaxTokens": 5, "Volume": 3}',
    [],
    [],
  ],
  [
    '{"MaxTokens": 5, "Volume": 3}',
    false,
    '{"MaxTokens": 5, "Volume": 3}',
    [],
    [],
  ],
  [
    '{"Controls": {"QuickAccessKey": "F3"}}',
    true,
    '{"Controls": {"QuickAccessKey": "F3"}}',
    [],
    [],
  ],
  [
    '{"Controls": {"QuickAccessKey": "F3"}}',
    false,
    '{"Controls": {"QuickAccessKey": "F3"}}',
    [],
    [],
  ],
  [
    '{"ToggleKey": "F5", "Session": "s3ss10n"}',
    false,
    '{"ToggleKey": "F5"}',
    [],
    ["Session"],
  ],
  // Secrets that never travel.
  [
    '{"ApiToken": "ghp_secret1", "ApiToken": ""}',
    false,
    '{"ApiToken": ""}',
    ["ApiToken"],
    [],
  ],
  [
    '{"ApiToken": {"Value": "ghp_secret2"}, "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["ApiToken"],
    [],
  ],
  ['{"Password": 123456}', true, "{}", ["Password"], []],
  [
    '{"AuthKey": "ak_secret3", "Path": "/home/sam/x"}',
    true,
    '{"Path": "/home/sam/x"}',
    ["AuthKey"],
    [],
  ],
  [
    '{"AuthKey": "ak_secret3", "Path": "/home/sam/x"}',
    false,
    "{}",
    ["AuthKey"],
    ["Path"],
  ],
  [
    '{"Recent": ["/home/sam/a", "ok"]}',
    false,
    '{"Recent": ["", "ok"]}',
    [],
    ["Recent[0]"],
  ],
  // Key bindings under key-like names are hotkeys, not credentials.
  [
    '{"MenuAccessKey": "RightThumbstickRight"}',
    false,
    '{"MenuAccessKey": "RightThumbstickRight"}',
    [],
    [],
  ],
  [
    '{"MenuAccessKey": "F3,LeftShoulder"}',
    true,
    '{"MenuAccessKey": "F3,LeftShoulder"}',
    [],
    [],
  ],
  [
    '{"MenuAccessKey": "LaunchApplication1"}',
    false,
    '{"MenuAccessKey": "LaunchApplication1"}',
    [],
    [],
  ],
  ['{"RevealSecret": "F9"}', false, '{"RevealSecret": "F9"}', [], []],
  ['{"RevealSecret": "F9"}', true, '{"RevealSecret": "F9"}', [], []],
  [
    '{"A": "LeftShift + F3", "AccessKey": "LeftShift+F3", "B": "DPadRight, RightShoulder"}',
    false,
    '{"A": "LeftShift + F3", "AccessKey": "LeftShift+F3", "B": "DPadRight, RightShoulder"}',
    [],
    [],
  ],
  // A button name under a password or token name is not a hotkey.
  [
    '{"Password": "Home", "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["Password"],
    [],
  ],
  ['{"ApiToken": "x", "Volume": 1}', false, '{"Volume": 1}', ["ApiToken"], []],
  [
    '{"Passwd": "pause+play", "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["Passwd"],
    [],
  ],
  // Anything else under them is a credential, kept flagged or not.
  [
    '{"AccessKey": "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY", "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["AccessKey"],
    [],
  ],
  [
    '{"AccessKey": "abcdefghijklmnop", "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["AccessKey"],
    [],
  ],
  [
    '{"AccessKey": 123456789012, "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["AccessKey"],
    [],
  ],
  [
    '{"SecretAccessKey": "dGhpcy+pc2FfYmFzZTY0", "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["SecretAccessKey"],
    [],
  ],
  [
    '{"AuthKey": "dGhpcy+pc2Ff", "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["AuthKey"],
    [],
  ],
  [
    '{"AuthKey": "correcthorsebattery", "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["AuthKey"],
    [],
  ],
  [
    '{"AuthKey": "Bearer abc123def456", "Volume": 1}',
    true,
    '{"Volume": 1}',
    ["AuthKey"],
    [],
  ],
  // A comment after the last field taken out goes with it.
  [
    '{"Volume": 3, "ApiToken": "ghp_last3" // ghp_last3\n}',
    false,
    '{"Volume": 3\n}',
    ["ApiToken"],
    [],
  ],
  [
    '{\n  "Volume": 3,\n  "ApiToken": "ghp_x" /* ghp_x */\n  // ghp_x again\n}\n',
    true,
    '{\n  "Volume": 3\n}\n',
    ["ApiToken"],
    [],
  ],
  // Comments, key order and number formatting are left as they are.
  [
    '{\n  // Cloud sync\n  "Zoom": 1,\n  "ApiToken": "ghp_abc",\n  "Alpha": 2.0,\n  "SaveBackupFolder": "/home/luke/Backups"\n}\n',
    false,
    '{\n  // Cloud sync\n  "Zoom": 1,\n  "Alpha": 2.0\n}\n',
    ["ApiToken"],
    ["SaveBackupFolder"],
  ],
];

describe("redaction shared with the backend", () => {
  it.each(SHARED_CASES)(
    "%s (keep flagged: %s)",
    (file, keep, shared, blocked, removed) => {
      const out = redactSettings(file, keep);
      expect(out.content).toBe(shared);
      expect(out.blocked).toEqual(blocked);
      expect(out.removed).toEqual(removed);
    },
  );
});

describe("files the redaction cannot read", () => {
  it("never shares them, as the backend refuses them", () => {
    for (const file of [
      '\uFEFF\uFEFF{"ApiToken": "ghp_bom2"}',
      '{"ApiToken": "ghp_e1", "Empty": [,]}',
      "[1, 2]",
    ])
      for (const keep of [false, true]) {
        const out = redactSettings(file, keep);
        expect(out.checked).toBe(false);
        expect(out.content).toBe("");
      }
  });
});

describe("deeply nested settings", () => {
  const nested = (depth: number) =>
    `{"a": ${"[".repeat(depth - 1)}${"]".repeat(depth - 1)}}`;
  it("reads up to the backend's depth limit and refuses anything deeper", () => {
    expect(MAX_DEPTH).toBe(128);
    expect(redactSettings(nested(MAX_DEPTH), false).checked).toBe(true);
    expect(redactSettings(nested(MAX_DEPTH + 1), false).checked).toBe(false);
    const deep = `{"ApiToken": "ghp_deep", "a": ${"[".repeat(50_000)}${"]".repeat(50_000)}}`;
    for (const keep of [false, true]) {
      const out = redactSettings(deep, keep);
      expect(out.checked).toBe(false);
      expect(out.content).toBe("");
    }
    expect(scanSettings(deep)).toBeNull();
  });

  it("does not compare settings nested past the limit", () => {
    const deep = `${'{"a": '.repeat(50_000)}1${"}".repeat(50_000)}`;
    expect(changedFields(deep, '{"a": 1}')).toBeNull();
    expect(changedFields('{"a": 1}', '{"a": 2}')).toEqual(["a"]);
  });
});
