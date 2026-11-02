import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ModsView } from "@/features/mods/ModsView";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

// Counts badge renders: every row draws one, so the count says how many rows
// a change re-rendered.
const badgeRenders = vi.hoisted(() => ({ count: 0 }));
vi.mock("@/components/ui/StatusBadge", () => ({
  StatusBadge: ({ children }: { children: React.ReactNode }) => {
    badgeRenders.count += 1;
    return <span>{children}</span>;
  },
}));

beforeEach(() => localStorage.clear());
afterEach(() => vi.restoreAllMocks());

const mod = (n: number) =>
  ({
    profile_component_id: `c${n}`,
    unique_id: `Filler.Mod${String(n).padStart(2, "0")}`,
    name: `Filler Mod ${String(n).padStart(2, "0")}`,
    author: "Pw",
    version: "1.0.0",
    description: null,
    enabled: true,
    installed_reason: "explicit",
    deployment_id: `d${n}`,
    artifact_hash: "h",
    installed_at: "",
  }) as unknown as ModListItemDto;

async function renderMods(count: number) {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p1", name: "Main", revision: 1 },
    game: { id: "g" },
  } as unknown as ProfileOverviewDto);
  vi.spyOn(api, "listProfileMods").mockResolvedValue(
    Array.from({ length: count }, (_, i) => mod(i + 1)),
  );
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ModsView />
    </QueryClientProvider>,
  );
  // Generous: a long list renders slowly when the test machine is busy.
  await screen.findByText("Filler Mod 01", undefined, { timeout: 10_000 });
}

const box = (n: number) =>
  screen.getByRole("checkbox", {
    name: `Select Filler Mod ${String(n).padStart(2, "0")}`,
  }) as HTMLInputElement;

describe("selecting mods for a bulk change", () => {
  it("selects every shown mod, and only those, at once", async () => {
    await renderMods(12);
    fireEvent.change(screen.getByRole("searchbox", { name: "Search mods" }), {
      target: { value: "mod 1" },
    });
    // Filler Mod 01, 10, 11, 12
    fireEvent.click(
      screen.getByRole("checkbox", { name: "Select all 4 shown" }),
    );
    expect(screen.getByText("4 selected")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search mods" }), {
      target: { value: "" },
    });
    expect(box(1).checked).toBe(true);
    expect(box(2).checked).toBe(false);
    expect(box(12).checked).toBe(true);

    const all = screen.getByRole("checkbox", {
      name: "Select all 12",
    }) as HTMLInputElement;
    expect(all.indeterminate).toBe(true);
    fireEvent.click(all);
    expect(screen.getByText("12 selected")).toBeInTheDocument();
    fireEvent.click(all);
    expect(screen.queryByText(/selected$/)).not.toBeInTheDocument();
  });

  it("selects a range with Shift+click", async () => {
    await renderMods(8);
    fireEvent.click(box(2));
    fireEvent.click(box(6), { shiftKey: true });
    expect([1, 2, 3, 4, 5, 6, 7].map((n) => box(n).checked)).toEqual([
      false,
      true,
      true,
      true,
      true,
      true,
      false,
    ]);
    // Shift+click on a ticked row clears from the last row clicked (6) to it.
    fireEvent.click(box(4), { shiftKey: true });
    expect([2, 3, 4, 5, 6].map((n) => box(n).checked)).toEqual([
      true,
      true,
      false,
      false,
      false,
    ]);
  });

  it("re-renders only the ticked row, not the whole list", async () => {
    await renderMods(40);
    badgeRenders.count = 0;
    fireEvent.click(box(30));
    expect(box(30).checked).toBe(true);
    // One row's badge, give or take; never the 40 rows.
    expect(badgeRenders.count).toBeLessThan(5);
  }, 20_000);
});
