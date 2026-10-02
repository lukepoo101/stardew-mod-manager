import { describe, expect, it } from "vitest";
import type { SmapiStatusDto } from "@/shared/api/generated";
import { smapiBadge, smapiExplanation } from "@/shared/smapi/status";

const status = (over: Partial<SmapiStatusDto>): SmapiStatusDto => ({
  is_installed: true,
  observed_version: "4.1.10",
  tested_version: "4.1.10",
  is_compatible: true,
  state: "installed",
  comparison: "same",
  evidence: [],
  ...over,
});

describe("SMAPI status wording", () => {
  it("never shows the tested version as the installed one", () => {
    expect(
      smapiBadge(status({ observed_version: null, comparison: "unknown" }))
        .label,
    ).toBe("SMAPI (version unknown)");
  });

  it("tells newer, older, partial and absent apart", () => {
    expect(
      smapiBadge(status({ observed_version: "4.2.0", comparison: "newer" })),
    ).toEqual({ label: "SMAPI 4.2.0, newer than tested", variant: "info" });
    expect(
      smapiBadge(status({ observed_version: "4.0.0", comparison: "older" }))
        .variant,
    ).toBe("warning");
    expect(smapiBadge(status({ state: "partial" })).label).toBe(
      "SMAPI incomplete",
    );
    expect(
      smapiBadge(status({ state: "absent", is_installed: false })).label,
    ).toBe("No SMAPI");
  });

  it("explains that newer is unverified, not broken, and is not downgraded", () => {
    const text = smapiExplanation(
      status({ observed_version: "4.2.0", comparison: "newer" }),
    );
    expect(text).toMatch(/not been verified with this manager/);
    expect(text).toMatch(/will not be downgraded/);
  });
});
