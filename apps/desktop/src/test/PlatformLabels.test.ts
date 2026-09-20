import { describe, expect, it } from "vitest";
import {
  installationLabel,
  operatingSystemLabel,
  storefrontLabel,
} from "@/shared/platform/labels";

describe("platform labels", () => {
  it("renders the lowercase contract values the backend sends", () => {
    expect(operatingSystemLabel("windows")).toBe("Windows");
    expect(operatingSystemLabel("linux")).toBe("Linux");
    expect(operatingSystemLabel("macos")).toBe("macOS");
  });

  it("never guesses a platform when the backend did not report one", () => {
    // A missing value must not silently become the host platform: the whole
    // point of the field is that the backend states it.
    expect(operatingSystemLabel(null)).toBe("Unknown platform");
    expect(operatingSystemLabel(undefined)).toBe("Unknown platform");
    expect(operatingSystemLabel("")).toBe("Unknown platform");
  });

  it("passes an unrecognised platform through instead of hiding it", () => {
    expect(operatingSystemLabel("plan9")).toBe("plan9");
  });

  it("labels a storefront without calling Steam native", () => {
    expect(storefrontLabel("steam")).toBe("Steam");
    expect(storefrontLabel("gog")).toBe("GOG");
    expect(storefrontLabel("manual")).toBe("Manual Folder");
  });

  it("describes the storefront together with the platform", () => {
    expect(installationLabel("steam", "windows")).toBe("Steam · Windows");
    expect(installationLabel("steam", null)).toBe("Steam");
  });
});
