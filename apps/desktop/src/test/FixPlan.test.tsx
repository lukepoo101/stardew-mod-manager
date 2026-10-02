import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
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
      await screen.findByRole("button", { name: "Enable 1 mod(s)" }),
    );
    await waitFor(() => expect(enable).toHaveBeenCalledWith(["c-Z.Lib"], true));
    expect(await screen.findByText(/Enabled Z\.Lib\./)).toBeInTheDocument();
  });
});
