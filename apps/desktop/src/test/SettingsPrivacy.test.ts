import { describe, expect, it } from "vitest";
import {
  changedFields,
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
