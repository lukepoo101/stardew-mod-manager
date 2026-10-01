import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LastSessionCard } from "@/features/overview/LastSessionCard";
import { api } from "@/shared/api/client";
import { describeSession, formatDuration } from "@/shared/launch/sessionResult";
import type { DiagnosticsDto, LaunchSessionDto } from "@/shared/api/generated";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

const session = (over: Partial<LaunchSessionDto> = {}): LaunchSessionDto => ({
  id: "s1",
  profile_id: "p1",
  launch_mode: "modded",
  state: "exited",
  launched_at: "2026-09-01T10:00:00Z",
  ended_at: "2026-09-01T10:42:00Z",
  pid: 1,
  verified_mods: ["A", "B", "C"],
  verification_details: null,
  game_version: "1.6.15",
  smapi_version: "4.1.10",
  ...over,
});

const report = (over: Partial<DiagnosticsDto> = {}) =>
  ({
    session_id: "s1",
    log_summary: {
      smapi_version: "4.1.10",
      game_version: "1.6.15",
      loaded_mod_count: 3,
      skipped_mods: [
        { name: "Broken", version: null, reason: "", missing_dependencies: [] },
      ],
      update_notices: [{}, {}],
      sources: [],
      total_lines: 10,
    },
    ...over,
  }) as unknown as DiagnosticsDto;

describe("post-game session result", () => {
  it("formats durations", () => {
    expect(formatDuration("2026-01-01T00:00:00Z", "2026-01-01T00:00:20Z")).toBe(
      "under a minute",
    );
    expect(formatDuration("2026-01-01T00:00:00Z", "2026-01-01T01:05:00Z")).toBe(
      "1 h 5 min",
    );
    expect(formatDuration("2026-01-01T00:00:00Z", null)).toBeNull();
  });

  it("ignores a session that is still running", () => {
    expect(
      describeSession(session({ state: "running_unverified", ended_at: null })),
    ).toBeNull();
    expect(
      describeSession(
        session({ state: "verification_unavailable", ended_at: null }),
      ),
    ).toBeNull();
  });

  it("uses log evidence only from the same session", () => {
    const own = describeSession(session(), report());
    expect(own?.title).toBe("SMAPI loaded 3 mod(s)");
    expect(own?.tone).toBe("warning");
    expect(own?.points).toEqual([
      "SMAPI skipped 1 mod(s): Broken.",
      "2 mod(s) reported an update.",
    ]);
    const other = describeSession(session(), report({ session_id: "older" }));
    expect(other?.points).toEqual([]);
    expect(other?.tone).toBe("success");
    expect(other?.detail).toMatch(/not that everything worked/);
  });

  it("distinguishes failed, vanilla, unverified and unobserved sessions", () => {
    expect(describeSession(session({ state: "failed" }))?.tone).toBe("danger");
    expect(
      describeSession(session({ launch_mode: "vanilla", verified_mods: [] }))
        ?.title,
    ).toBe("Played without mods");
    expect(describeSession(session({ verified_mods: [] }))?.title).toBe(
      "Could not confirm that mods loaded",
    );
    expect(
      describeSession(session({ state: "exited", ended_at: null }))?.title,
    ).toBe("The manager did not see the game close");
  });

  it("can be hidden without affecting history", async () => {
    vi.spyOn(api, "getLatestLaunchSession").mockResolvedValue(session());
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <LastSessionCard />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText("SMAPI loaded 3 mod(s)"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Started on Stardew Valley 1\.6\.15, SMAPI 4\.1\.10/),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Hide last session result" }),
    );
    expect(screen.queryByText("SMAPI loaded 3 mod(s)")).toBeNull();
    expect(localStorage.getItem("smm-dismissed-session")).toBe("s1");
  });
});
