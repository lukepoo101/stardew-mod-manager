import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LastSessionCard } from "@/features/overview/LastSessionCard";
import { api } from "@/shared/api/client";
import type { DiagnosticsDto, LaunchSessionDto } from "@/shared/api/generated";
import {
  describeRunningSession,
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
  outcome: null,
  skipped_mods: [],
  unconfirmed_mods: [],
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
        state: "verification_unavailable",
        evidence: "smapi_started",
        outcome: "unavailable",
        verified_mods: [],
      }),
    );
    expect(unknown?.tone).toBe("warning");
    expect(unknown?.title).toBe("Could not confirm that SMAPI started");
  });

  it("report success for a confirmed test run that has ended", () => {
    // A confirmed session that ends is "exited"; the evidence decides.
    const ended = describeSession(
      session({
        launch_mode: "runtime_test",
        state: "exited",
        evidence: "mods_loaded",
        outcome: "confirmed",
        verified_mods: [],
      }),
    );
    expect(ended?.tone).toBe("success");
    expect(ended?.title).toBe("SMAPI started on its own");
  });
});

describe("partial and unverifiable sessions", () => {
  const partial = session({
    state: "exited",
    evidence: "smapi_started",
    outcome: "partial",
    verified_mods: ["A", "B"],
    expected_mods: ["A", "B", "Weather.Plus"],
    skipped_mods: [
      {
        name: "Weather Plus 3.1.0",
        reason: "it's no longer compatible.",
      },
    ],
    verification_details: "SMAPI skipped Weather Plus 3.1.0.",
  });

  it("names a skipped mod with SMAPI's reason instead of calling it a failed start", () => {
    const result = describeSession(partial);
    expect(result?.tone).toBe("warning");
    expect(result?.title).toBe("SMAPI skipped 1 mod(s)");
    expect(result?.title).not.toMatch(/did not start/);
    expect(result?.detail).toMatch(/confirmed 2 of 3 mod\(s\) loaded/);
    expect(result?.points).toContain(
      "SMAPI skipped Weather Plus 3.1.0 because it's no longer compatible.",
    );
  });

  it("says unavailable, not failure or success, when the log cannot tell", () => {
    const result = describeSession(
      session({
        state: "verification_unavailable",
        evidence: "smapi_started",
        outcome: "unavailable",
        verified_mods: [],
        verification_details:
          "SMAPI's log does not show which Mods folder it used.",
      }),
    );
    expect(result?.tone).toBe("neutral");
    expect(result?.title).toBe("Verification unavailable");
  });

  it("names another Mods folder as the problem", () => {
    const result = describeSession(
      session({ state: "failed", outcome: "wrong_mods_folder" }),
    );
    expect(result?.tone).toBe("danger");
    expect(result?.title).toBe("SMAPI used a different Mods folder");
  });

  it("shows the running state in the colour of its evidence", () => {
    const running = { ...partial, state: "running_unverified", ended_at: null };
    expect(describeRunningSession(running)?.tone).toBe("warning");
    expect(describeRunningSession(running)?.points[0]).toMatch(
      /Weather Plus 3\.1\.0/,
    );
    expect(describeRunningSession({ ...running, outcome: null })?.tone).toBe(
      "info",
    );
    expect(
      describeRunningSession({
        ...running,
        outcome: "confirmed",
        skipped_mods: [],
      })?.tone,
    ).toBe("success");
    expect(
      describeRunningSession({ ...running, launch_mode: "vanilla" }),
    ).toBeNull();
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
