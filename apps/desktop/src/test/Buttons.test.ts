import { describe, expect, it } from "bun:test";
import { isKeybind, SMAPI_BUTTONS } from "@/shared/recipe/buttons";

/** Shared word for word with `manager-core::smapi::buttons`
 * (KEYBIND_CASES): [text, is a key binding]. */
const KEYBIND_CASES: [string, boolean][] = [
  ["F3", true],
  ["RightThumbstickRight", true],
  ["F3,LeftShoulder", true],
  ["LeftShift + F3", true],
  ["LeftShift+F3", true],
  ["DPadRight, RightShoulder", true],
  ["LaunchApplication1", true],
  ["f9", true],
  ["", false],
  ["F25", false],
  ["Bearer abc123def456", false],
  ["correcthorsebattery", false],
  ["abcdefghijklmnop", false],
  ["wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY", false],
  ["123456789012", false],
];

describe("SMAPI key bindings", () => {
  it.each(KEYBIND_CASES)("%s is a key binding: %s", (text, expected) => {
    expect(isKeybind(text)).toBe(expected);
  });

  it("lists keyboard, controller and mouse buttons once each", () => {
    expect(new Set(SMAPI_BUTTONS).size).toBe(SMAPI_BUTTONS.length);
    for (const name of ["F24", "RightThumbstickRight", "MouseX2", "OemEnlW"])
      expect(SMAPI_BUTTONS).toContain(name);
  });
});
