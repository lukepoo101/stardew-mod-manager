import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FixPlanCard } from "@/features/overview/FixPlanCard";
import { api } from "@/shared/api/client";
import type {
  FindingDto,
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import { planFixes } from "@/shared/health/fixPlan";

afterEach(() => vi.restoreAllMocks());

const finding = (code: string, title: string): FindingDto =>
  ({
    id: title,
    fingerprint: title,
    code,
    severity: "error",
    category: "dependency",
    title,
    summary: `${title} (summary)`,
    affected_entities: [],
    evidence: [],
    observed_at: "",
  }) as FindingDto;

const mod = (id: string, enabled: boolean) =>
  ({
    profile_component_id: `c-${id}`,
    unique_id: id,
    name: id,
    version: "1.0",
    enabled,
  }) as ModListItemDto;

describe("planning fixes", () => {
  it("enables a disabled requirement and leaves the rest with reasons", () => {
    const plan = planFixes(
      [
        finding(
          "DEPENDENCY_DISABLED",
          "Required dependency 'Z.Lib' is disabled",
        ),
        finding("MISSING_DEPENDENCY", "Missing Required Dependency 'Gone'"),
        finding("DUPLICATE_UNIQUE_ID", "2 enabled mods share the ID 'A'"),
        finding("SMAPI_MISSING", "SMAPI Not Installed"),
      ],
      [mod("z.lib", false), mod("A.Needy", true)],
    );
    expect(plan.fixes.map((f) => f.mod.unique_id)).toEqual(["z.lib"]);
    expect(plan.leftAlone.map((l) => l.finding.code)).toEqual([
      "MISSING_DEPENDENCY",
      "DUPLICATE_UNIQUE_ID",
    ]);
  });

  it("does not guess between two disabled copies", () => {
    const plan = planFixes(
      [
        finding(
          "DEPENDENCY_DISABLED",
          "Required dependency 'Z.Lib' is disabled",
        ),
      ],
      [
        { ...mod("Z.Lib", false), profile_component_id: "a" },
        { ...mod("Z.Lib", false), profile_component_id: "b" },
      ],
    );
    expect(plan.fixes).toEqual([]);
    expect(plan.leftAlone[0].reason).toMatch(/choose which to enable/);
  });
});

describe("the fixes card", () => {
  it("enables the planned mods after confirmation by button", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
      health_summary: {
        findings: [
          finding(
            "DEPENDENCY_DISABLED",
            "Required dependency 'Z.Lib' is disabled",
          ),
        ],
      },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([mod("Z.Lib", false)]);
    const enable = vi
      .spyOn(api, "setModsEnabled")
      .mockResolvedValue({ changed: ["Z.Lib"], failed: [] });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <FixPlanCard />
      </QueryClientProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Make 1 fix(es)" }),
    );
    await waitFor(() => expect(enable).toHaveBeenCalledWith(["c-Z.Lib"], true));
    expect(await screen.findByText(/Enabled Z\.Lib\./)).toBeInTheDocument();
  });
});

import { missingRequirement, uniqueStoredFix } from "@/shared/health/fixPlan";

describe("installing a missing requirement from storage", () => {
  const missing = {
    ...finding("MISSING_DEPENDENCY", "Missing Required Dependency 'Z.Lib'"),
    summary:
      "Mod 'Needy' requires 'Z.Lib' 2.0.0 or newer, but it is not installed.",
  } as FindingDto;

  it("reads the requirement and only accepts a single fitting copy", () => {
    expect(missingRequirement(missing)).toEqual({
      uniqueId: "Z.Lib",
      minimum: "2.0.0",
    });
    const copy = (version: string, meets: boolean | null) => ({
      version,
      meets_minimum: meets,
    });
    expect(
      uniqueStoredFix([copy("2.1", true), copy("1.0", false)])?.version,
    ).toBe("2.1");
    expect(uniqueStoredFix([copy("2.1", true), copy("2.2", true)])).toBeNull();
    expect(uniqueStoredFix([copy("?", null)])).toBeNull();
  });

  it("offers and installs the single stored copy", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
      health_summary: { findings: [missing] },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([]);
    vi.spyOn(api, "findStoredMod").mockResolvedValue([
      {
        artifact_hash: "h1",
        name: "Z Lib",
        version: "2.1.0",
        original_filename: "ZLib.zip",
        meets_minimum: true,
      },
    ]);
    const install = vi.spyOn(api, "installStoredPackage").mockResolvedValue();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <FixPlanCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(
        /Install Z Lib 2.1.0 from the copy the manager stores/,
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Make 1 fix(es)" }));
    await waitFor(() => expect(install).toHaveBeenCalledWith("p1", "h1", true));
    expect(
      await screen.findByText(/Installed Z Lib 2.1.0/),
    ).toBeInTheDocument();
  });
});
