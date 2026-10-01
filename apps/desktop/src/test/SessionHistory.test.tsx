import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SessionHistoryCard } from "@/features/diagnostics/SessionHistoryCard";
import { api } from "@/shared/api/client";
import type { LaunchSessionDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const session = (over: Partial<LaunchSessionDto>): LaunchSessionDto => ({
  id: "s",
  profile_id: "p1",
  launch_mode: "modded",
  state: "exited",
  launched_at: "2026-09-01T10:00:00Z",
  ended_at: "2026-09-01T10:30:00Z",
  pid: 1,
  verified_mods: ["A", "B"],
  verification_details: null,
  game_version: "1.6.15",
  smapi_version: "4.1.10",
  acknowledged_warnings: [],
  ...over,
});

const renderCard = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SessionHistoryCard />
    </QueryClientProvider>,
  );

describe("session history", () => {
  it("lists each session with its result, runtime and evidence", async () => {
    vi.spyOn(api, "listLaunchSessions").mockResolvedValue([
      session({
        id: "failed",
        state: "failed",
        ended_at: "2026-09-02T10:00:05Z",
        launched_at: "2026-09-02T10:00:00Z",
        verified_mods: [],
        verification_details: "The game could not be started: no permission",
        game_version: null,
      }),
      session({
        id: "ok",
        acknowledged_warnings: ["The last launch was not verified"],
      }),
      session({
        id: "vanilla",
        launch_mode: "vanilla",
        verified_mods: [],
      }),
    ]);
    renderCard();
    expect(
      await screen.findByText("The game did not start properly"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /Started on Stardew Valley \(unknown\), SMAPI 4\.1\.10 with 0 mod/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/The game could not be started: no permission/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /Started on Stardew Valley 1\.6\.15, SMAPI 4\.1\.10 with 2 mod/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Started on Stardew Valley 1.6.15."),
    ).toBeInTheDocument();
  });

  it("says when there are no sessions yet", async () => {
    vi.spyOn(api, "listLaunchSessions").mockResolvedValue([]);
    renderCard();
    expect(
      await screen.findByText(/No game sessions have been recorded/),
    ).toBeInTheDocument();
  });
});
