import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RestorePointsCard } from "@/features/profiles/RestorePointsCard";
import { api } from "@/shared/api/client";
import type { ProfileOverviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

function renderCard() {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p1" },
  } as unknown as ProfileOverviewDto);
  vi.spyOn(api, "listRestorePoints").mockResolvedValue([
    {
      id: "rp1",
      label: "Working",
      created_at: "2026-09-01T10:00:00Z",
      mods: [],
      operations: ["op-remove", "op-install"],
    },
  ]);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RestorePointsCard />
    </QueryClientProvider>,
  );
}

describe("restore points", () => {
  it("offers the last working setup as a full restore", async () => {
    vi.spyOn(api, "getKnownGood").mockResolvedValue({
      profile_id: "p1",
      recorded_at: "2026-09-01T10:00:00Z",
      game_version: null,
      smapi_version: null,
      mods: [],
      findings: null,
    });
    const plan = vi.spyOn(api, "planRestore").mockResolvedValue({
      point_id: "known-good",
      available: true,
      unavailable: [],
      remove: [],
      install: [],
      change_version: ["K.Mod 2.0.0 → 1.0.0"],
      enable: [],
      disable: [],
    });
    renderCard();
    expect(
      await screen.findByText("The last working setup"),
    ).toBeInTheDocument();
    const reviews = screen.getAllByRole("button", { name: "Review restore" });
    fireEvent.click(reviews[0]);
    await waitFor(() => expect(plan).toHaveBeenCalledWith("p1", "known-good"));
    expect(await screen.findByText("K.Mod 2.0.0 → 1.0.0")).toBeInTheDocument();
  });

  it("says which change an automatic point was taken before", async () => {
    renderCard();
    expect(
      await screen.findByText(/saved before 2 change\(s\) shown in Activity/),
    ).toBeInTheDocument();
  });

  it("shows the plan before restoring and restores on confirmation", async () => {
    vi.spyOn(api, "planRestore").mockResolvedValue({
      point_id: "rp1",
      available: true,
      unavailable: [],
      remove: ["Y.New 1.0.0"],
      install: [],
      change_version: ["V.Mod 2.0.0 → 1.0.0"],
      enable: [],
      disable: [],
    });
    const restore = vi.spyOn(api, "restoreToPoint").mockResolvedValue({
      undo_point_id: "rp2",
      done: [],
      failed: [],
    });
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Review restore" }),
    );
    expect(await screen.findByText("Y.New 1.0.0")).toBeInTheDocument();
    expect(screen.getByText("V.Mod 2.0.0 → 1.0.0")).toBeInTheDocument();
    expect(restore).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(restore).toHaveBeenCalledWith("p1", "rp1"));
    expect(await screen.findByText('Restored "Working".')).toBeInTheDocument();
  });

  it("offers no restore when a needed package is gone", async () => {
    vi.spyOn(api, "planRestore").mockResolvedValue({
      point_id: "rp1",
      available: false,
      unavailable: ["G.Mod 1.0.0: its package is no longer kept intact"],
      remove: [],
      install: [],
      change_version: [],
      enable: [],
      disable: [],
    });
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Review restore" }),
    );
    expect(
      await screen.findByText(/its package is no longer kept intact/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restore" })).toBeNull();
  });
});
