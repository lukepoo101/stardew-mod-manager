import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GameVersionCard } from "@/features/settings/GameVersionCard";
import { api } from "@/shared/api/client";
import type { ProfileOverviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const show = () => {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p1" },
    game: { id: "g1" },
  } as unknown as ProfileOverviewDto);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <GameVersionCard />
    </QueryClientProvider>,
  );
};

describe("setting the game version", () => {
  it("sets a version with a reason when detection cannot read one", async () => {
    vi.spyOn(api, "getGameVersionOverride").mockResolvedValue({
      detected: null,
      value: null,
      reason: null,
      set_at: null,
      detected_then: null,
      stale: false,
    });
    const set = vi.spyOn(api, "setGameVersionOverride").mockResolvedValue();
    show();
    expect(
      await screen.findByText("Detected: could not be read."),
    ).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("e.g. 1.6.15"), {
      target: { value: "1.6.15" },
    });
    fireEvent.change(screen.getByLabelText("Why (optional)"), {
      target: { value: "Unreadable" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Use this version" }));
    await waitFor(() =>
      expect(set).toHaveBeenCalledWith("g1", "1.6.15", "Unreadable"),
    );
  });

  it("shows the override as the user's, flags it when stale and can clear it", async () => {
    vi.spyOn(api, "getGameVersionOverride").mockResolvedValue({
      detected: "1.6.16",
      value: "1.6.15",
      reason: "Detection was wrong",
      set_at: "2026-10-01T10:00:00Z",
      detected_then: "1.6.14",
      stale: true,
    });
    const set = vi.spyOn(api, "setGameVersionOverride").mockResolvedValue();
    show();
    expect(await screen.findByText(/Set by you:/)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      /Detection has changed since you set it/,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Use the detected version again" }),
    );
    await waitFor(() => expect(set).toHaveBeenCalledWith("g1", null, ""));
  });
});
