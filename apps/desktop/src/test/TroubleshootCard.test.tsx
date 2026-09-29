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
      await screen.findByText(/Your current setup is recorded first/),
    ).toBeInTheDocument();
    const start = vi
      .spyOn(api, "startTroubleshoot")
      .mockResolvedValue(state({}));
    fireEvent.click(
      screen.getByRole("button", { name: "Start troubleshooting" }),
    );
    await waitFor(() => expect(start).toHaveBeenCalled());
    expect(await screen.findByText(/Every mod is now off/)).toBeInTheDocument();
  });

  it("sends each answer and shows the next step", async () => {
    renderCard(state({ phase: "all_off" }));
    const answer = vi
      .spyOn(api, "answerTroubleshoot")
      .mockResolvedValue(
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
      await screen.findByRole("button", { name: "Start troubleshooting" }),
    ).toBeInTheDocument();
  });

  it("shows a failed step instead of hiding it", async () => {
    renderCard(state({ active: false, phase: "inactive" }));
    vi.spyOn(api, "startTroubleshoot").mockRejectedValue(new Error("boom"));
    fireEvent.click(
      await screen.findByRole("button", { name: "Start troubleshooting" }),
    );
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });
});
