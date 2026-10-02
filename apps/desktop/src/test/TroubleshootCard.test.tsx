import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TroubleshootCard } from "@/features/diagnostics/TroubleshootCard";
import { api } from "@/shared/api/client";
import type { TroubleshootDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const state = (over: Partial<TroubleshootDto>): TroubleshootDto => ({
  active: true,
  phase: "all_off",
  step: 0,
  suspects: [],
  enabled_mods: [],
  culprit: null,
  note: null,
  together: [],
  history: [],
  ...over,
});

function renderCard(initial: TroubleshootDto) {
  vi.spyOn(api, "getTroubleshootStatus").mockResolvedValue(initial);
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <TroubleshootCard />
    </QueryClientProvider>,
  );
}

describe("troubleshooting card", () => {
  it("explains what will happen before anything is changed", async () => {
    renderCard(state({ active: false, phase: "inactive" }));
    expect(
      await screen.findByText(/the original is never\s+changed/),
    ).toBeInTheDocument();
    const start = vi
      .spyOn(api, "startTroubleshoot")
      .mockResolvedValue(state({}));
    fireEvent.click(
      screen.getByRole("button", {
        name: "Troubleshoot this profile directly",
      }),
    );
    await waitFor(() => expect(start).toHaveBeenCalled());
    expect(await screen.findByText(/Every mod is now off/)).toBeInTheDocument();
  });

  it("sends each answer and shows the next step", async () => {
    renderCard(state({ phase: "all_off" }));
    const answer = vi.spyOn(api, "answerTroubleshoot").mockResolvedValue(
      state({
        phase: "testing",
        step: 1,
        enabled_mods: ["Alpha", "Lib"],
        suspects: ["Alpha", "Lib", "Beta"],
      }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "The problem is gone" }),
    );
    await waitFor(() => expect(answer).toHaveBeenCalledWith(false));
    expect(
      await screen.findByText(/2 mod\(s\) are on, 3 still under suspicion/),
    ).toBeInTheDocument();
  });

  it("names the likely culprit with its caveat and offers restore", async () => {
    renderCard(
      state({
        phase: "found",
        culprit: "Alpha",
        note: "assuming a single mod",
      }),
    );
    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText(/assuming a single mod/)).toBeInTheDocument();
    const restore = vi
      .spyOn(api, "restoreTroubleshoot")
      .mockResolvedValue(state({ active: false, phase: "inactive" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Restore my original mods" }),
    );
    await waitFor(() => expect(restore).toHaveBeenCalled());
    expect(
      await screen.findByRole("button", {
        name: "Troubleshoot this profile directly",
      }),
    ).toBeInTheDocument();
  });

  it("shows a failed step instead of hiding it", async () => {
    renderCard(state({ active: false, phase: "inactive" }));
    vi.spyOn(api, "startTroubleshoot").mockRejectedValue(new Error("boom"));
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Troubleshoot this profile directly",
      }),
    );
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });
});

describe("troubleshooting on a copy", () => {
  it("copies the profile, switches to it and starts there", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Main" },
    } as never);
    vi.spyOn(api, "listExperiments").mockResolvedValue([]);
    const copy = vi.spyOn(api, "startExperiment").mockResolvedValue({
      profile_id: "p2",
      profile_name: "Main troubleshooting",
    } as never);
    const activate = vi.spyOn(api, "activateProfile").mockResolvedValue();
    const start = vi
      .spyOn(api, "startTroubleshoot")
      .mockResolvedValue(state({ phase: "all_off" }));
    renderCard(state({ active: false, phase: "inactive" }));
    const button = await screen.findByRole("button", {
      name: "Start on a copy",
    });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    await waitFor(() => expect(start).toHaveBeenCalled());
    expect(copy).toHaveBeenCalledWith("p1", "Main troubleshooting");
    expect(activate).toHaveBeenCalledWith("p2");
    expect(activate.mock.invocationCallOrder[0]).toBeLessThan(
      start.mock.invocationCallOrder[0],
    );
  });

  it("lists answered steps with their sessions and explains grouping", async () => {
    renderCard(
      state({
        phase: "testing",
        step: 2,
        enabled_mods: ["Alpha", "Lib"],
        suspects: ["Alpha"],
        together: ["Lib stay on while Alpha is on, because it needs them."],
        history: [
          {
            step: 0,
            mods_on: 0,
            problem_present: false,
            answered_at: "2026-10-02T10:00:00Z",
            session_id: "s1",
            session_state: "mod_load_confirmed",
          },
        ],
      }),
    );
    expect(
      await screen.findByText(/because it needs them/),
    ).toBeInTheDocument();
    expect(screen.getByText(/0 mod\(s\) on: no problem/)).toBeInTheDocument();
    expect(screen.getByText(/mod load confirmed/)).toBeInTheDocument();
  });
});
