import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LastSessionCard } from "@/features/overview/LastSessionCard";
import { api } from "@/shared/api/client";
import type { DiagnosticsDto, LaunchSessionDto } from "@/shared/api/generated";
import {
  describeSession,
  formatDuration,
  isRunningState,
} from "@/shared/launch/sessionResult";

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
  acknowledged_warnings: [],
  expected_mods: [],
  evidence: "mods_loaded",
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
      errors: [],
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

  it("says when the end of a session was not observed", () => {
    const interrupted = session({ state: "interrupted", ended_at: null });
    expect(isRunningState("interrupted")).toBe(false);
    expect(isRunningState("running_unverified")).toBe(true);
    expect(describeSession(interrupted)?.title).toBe(
      "The manager did not see the game close",
    );
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

describe("SMAPI test runs", () => {
  it("say only SMAPI was tested, never that the mods work", () => {
    const ok = describeSession(
      session({
        launch_mode: "runtime_test",
        state: "mod_load_confirmed",
        verified_mods: [],
      }),
    );
    expect(ok?.title).toBe("SMAPI started on its own");
    expect(ok?.detail).toMatch(/does not show that your mods work/);
    const unknown = describeSession(
      session({
        launch_mode: "runtime_test",
        state: "exited",
        verified_mods: [],
      }),
    );
    expect(unknown?.tone).toBe("warning");
    expect(unknown?.title).toBe("Could not confirm that SMAPI started");
  });
});

import { compareLogs } from "@/shared/launch/compareSessions";

describe("comparing two sessions' logs", () => {
  const log = (skipped: string[], erroring: string[]) =>
    ({
      log_summary: {
        skipped_mods: skipped.map((name) => ({ name })),
        sources: erroring.map((source) => ({ source, errors: 1, warnings: 0 })),
      },
    }) as unknown as DiagnosticsDto;

  it("names what changed in skipped mods and error sources", () => {
    expect(compareLogs(log(["Old"], ["A"]), log(["New"], ["A", "B"]))).toEqual({
      newlySkipped: ["New"],
      noLongerSkipped: ["Old"],
      newErrorSources: ["B"],
      goneErrorSources: [],
    });
  });
});
