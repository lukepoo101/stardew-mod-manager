import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ErrorsAroundOperation } from "@/features/activity/ErrorsAroundOperation";
import { api } from "@/shared/api/client";
import type { DiagnosticsDto, LaunchSessionDto } from "@/shared/api/generated";
import { sessionsAround } from "@/shared/launch/operationErrors";

afterEach(() => vi.restoreAllMocks());

const session = (
  id: string,
  at: string,
  over: Partial<LaunchSessionDto> = {},
) =>
  ({
    id,
    profile_id: "p1",
    launch_mode: "modded",
    launched_at: at,
    ...over,
  }) as LaunchSessionDto;

const report = (errorSources: string[], skipped: string[] = [], saved = true) =>
  ({
    log_is_saved_copy: saved,
    log_summary: {
      skipped_mods: skipped.map((name) => ({ name })),
      sources: errorSources.map((source) => ({ source, errors: 2 })),
    },
  }) as unknown as DiagnosticsDto;

describe("sessions around an operation", () => {
  it("picks the modded sessions of that profile just before and after", () => {
    const sessions = [
      session("old", "2026-09-01T09:00:00Z"),
      session("before", "2026-09-01T10:00:00Z"),
      session("vanilla", "2026-09-01T11:30:00Z", { launch_mode: "vanilla" }),
      session("other", "2026-09-01T11:40:00Z", { profile_id: "p2" }),
      session("after", "2026-09-01T12:00:00Z"),
      session("later", "2026-09-01T13:00:00Z"),
    ];
    const around = sessionsAround(sessions, "p1", "2026-09-01T11:00:00Z");
    expect(around.before?.id).toBe("before");
    expect(around.after?.id).toBe("after");
    expect(sessionsAround(sessions, "p1", "2026-09-01T14:00:00Z").after).toBe(
      null,
    );
  });
});

const show = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ErrorsAroundOperation
        profileId="p1"
        at="2026-09-01T11:00:00Z"
        changed={["Fancy"]}
      />
    </QueryClientProvider>,
  );

describe("errors around an operation", () => {
  it("separates errors that first appeared after it from earlier ones", async () => {
    vi.spyOn(api, "listLaunchSessions").mockResolvedValue([
      session("before", "2026-09-01T10:00:00Z"),
      session("after", "2026-09-01T12:00:00Z"),
    ]);
    vi.spyOn(api, "getDiagnosticsReport").mockImplementation(
      async (_game, id) =>
        id === "before" ? report(["Old"]) : report(["Old", "Fancy"], ["Other"]),
    );
    show();
    expect(
      await screen.findByText(
        "New since the session before: Fancy (changed here) logged errors",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("New since the session before: Other was skipped"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Already logging errors before: Old/),
    ).toBeInTheDocument();
    expect(screen.getByText(/not proof that it caused/)).toBeInTheDocument();
  });

  it("says it cannot compare without a session or saved log on both sides", async () => {
    vi.spyOn(api, "listLaunchSessions").mockResolvedValue([
      session("after", "2026-09-01T12:00:00Z"),
    ]);
    show();
    expect(
      await screen.findByText(/there is no game session before it/),
    ).toBeInTheDocument();
  });

  it("does not compare against a log that was not saved", async () => {
    vi.spyOn(api, "listLaunchSessions").mockResolvedValue([
      session("before", "2026-09-01T10:00:00Z"),
      session("after", "2026-09-01T12:00:00Z"),
    ]);
    vi.spyOn(api, "getDiagnosticsReport").mockImplementation(
      async (_game, id) => report([], [], id === "after"),
    );
    show();
    expect(
      await screen.findByText(/a saved SMAPI log is missing/),
    ).toBeInTheDocument();
  });
});
