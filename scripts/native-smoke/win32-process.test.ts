// Tests for the parts of the Windows process launcher that do not need
// Windows: the command line and the environment block CreateProcessW is given.
// Run with `bun test ./scripts`.
import { describe, expect, test } from "bun:test";
import { commandLine, environmentBlock } from "./win32-process.ts";
import { failureResult } from "./windows.ts";

// Each case is an argument list and what subprocess.list2cmdline made of it
// (Python 3.13), which is what Popen passed to CreateProcessW.
const LIST2CMDLINE: Array<[string[], string]> = [
  [
    ["C:\\Program Files\\Stardew Mod Manager\\stardew-mod-manager.exe"],
    '"C:\\Program Files\\Stardew Mod Manager\\stardew-mod-manager.exe"',
  ],
  [["plain"], "plain"],
  [[""], '""'],
  [['a"b'], 'a\\"b'],
  [["with space\\"], '"with space\\\\"'],
  [["C:\\dir\\"], "C:\\dir\\"],
  [['back\\\\"quote'], 'back\\\\\\\\\\"quote'],
  [["tab\there"], '"tab\there"'],
  [
    [
      "tauri-driver",
      "--port=1",
      "--native-driver",
      "C:\\Edge Driver\\msedgedriver.exe",
    ],
    'tauri-driver --port=1 --native-driver "C:\\Edge Driver\\msedgedriver.exe"',
  ],
];

describe("commandLine", () => {
  test.each(LIST2CMDLINE)("%j quotes as list2cmdline did", (argv, expected) => {
    expect(commandLine(argv)).toBe(expected);
  });
});

describe("environmentBlock", () => {
  test("is sorted without regard to case and ends with two NULs", () => {
    const block = environmentBlock({ b: "2", A: "1", C: "x=y" });
    expect(Buffer.from(block).toString("utf16le")).toBe("A=1\0b=2\0C=x=y\0\0");
  });

  test("keeps characters outside ASCII", () => {
    const block = environmentBlock({ PATHÉ: "C:\\Ünïcode" });
    expect(Buffer.from(block).toString("utf16le")).toBe(
      "PATHÉ=C:\\Ünïcode\0\0",
    );
  });
});

describe("failureResult", () => {
  test("carries the error as the reason the workflow reports", () => {
    const result = failureResult(new Error("SMAPI installer exited"), false);
    expect(result).toEqual({
      status: "failed",
      reason: "SMAPI installer exited",
      driver_provider: null,
    });
  });

  test("names no provider for a direct start", () => {
    expect(failureResult(new Error("exited"), true)).toEqual({
      status: "failed",
      reason: "exited",
    });
  });
});
