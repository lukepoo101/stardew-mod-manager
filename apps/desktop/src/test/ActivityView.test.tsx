import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import {
  ActivityView,
  recordedMessage,
} from "@/features/activity/ActivityView";
import { StartupRecoveryNotice } from "@/features/recovery/StartupRecoveryNotice";
import { api } from "@/shared/api/client";
import { savePreferences, DEFAULT_PREFERENCES } from "@/shared/preferences";
import type { OperationDto } from "@/shared/api/generated";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

function wrap(ui: React.ReactNode) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <MemoryRouter>{ui}</MemoryRouter>
    </QueryClientProvider>,
  );
}

function op(overrides: Partial<OperationDto>): OperationDto {
  return {
    id: "op-1",
    kind: "mod_install",
    state: "succeeded",
    game_installation_id: null,
    profile_id: "p1",
    progress_current: null,
    progress_total: null,
    error_code: null,
    error_message: null,
    created_at: "2026-09-01T10:00:00Z",
    updated_at: "2026-09-01T10:00:00Z",
    completed_at: "2026-09-01T10:00:00Z",
    rolled_back: false,
    part_of: null,
    title: "Install Content Patcher 2.0.0",
    closed_preview: false,
    ...overrides,
  };
}

describe("Activity", () => {
  it("titles entries in plain words and keeps ids for Expert mode", async () => {
    vi.spyOn(api, "listRecentOperations").mockResolvedValue([op({})]);
    wrap(<ActivityView />);
    expect(
      await screen.findByText("Install Content Patcher 2.0.0"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/op-1/)).toBeNull();
    expect(screen.queryByText("mod_install")).toBeNull();
    expect(screen.queryByText(/Audit trail/)).toBeNull();
  });

  it("shows ids and raw kinds in Expert mode", async () => {
    savePreferences({ ...DEFAULT_PREFERENCES, expertMode: true });
    vi.spyOn(api, "listRecentOperations").mockResolvedValue([op({})]);
    wrap(<ActivityView />);
    expect(
      await screen.findByText(/mod_install · succeeded · op-1/),
    ).toBeInTheDocument();
  });

  it("leaves out reviews that were closed without changing anything", async () => {
    vi.spyOn(api, "listRecentOperations").mockResolvedValue([
      op({}),
      op({
        id: "op-2",
        state: "cancelled",
        title: "Install Big Map 1.0.0",
        closed_preview: true,
        error_message: "Cancelled by user",
      }),
    ]);
    wrap(<ActivityView />);
    await screen.findByText("Install Content Patcher 2.0.0");
    expect(screen.queryByText("Install Big Map 1.0.0")).toBeNull();
    expect(screen.queryByText(/Error: Cancelled by user/)).toBeNull();
    expect(
      screen.getByText(/1 review\(s\) closed without changes/),
    ).toBeInTheDocument();
  });

  it("shows a recovery note on a finished entry as a note, not an error", async () => {
    vi.spyOn(api, "listRecentOperations").mockResolvedValue([
      op({
        error_code: "RECOVERED_SMAPI_STATE",
        error_message: "Recovered managed SMAPI state from filesystem evidence",
      }),
    ]);
    wrap(<ActivityView />);
    expect(
      await screen.findByText(
        "Recovered managed SMAPI state from filesystem evidence",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/^Error:/)).toBeNull();
  });

  it("offers a way out of an open review and of an operation waiting for recovery", async () => {
    vi.spyOn(api, "listRecentOperations").mockResolvedValue([
      op({ id: "d", state: "draft", title: "Install Left 1.0.0" }),
      op({
        id: "r",
        state: "recovery_required",
        title: "Remove Lib",
        error_message: '{"code":"X","message":"Both copies exist"}',
      }),
    ]);
    const cancel = vi.spyOn(api, "cancelActiveOperation").mockResolvedValue();
    const retry = vi.spyOn(api, "retryRecovery").mockResolvedValue();
    wrap(<ActivityView />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Discard this review" }),
    );
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("d"));
    expect(screen.getByText("Error: Both copies exist")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry recovery" }));
    await waitFor(() => expect(retry).toHaveBeenCalled());
  });

  it("can show older entries", async () => {
    const list = vi
      .spyOn(api, "listRecentOperations")
      .mockResolvedValue(
        Array.from({ length: 50 }, (_, i) => op({ id: `op-${i}` })),
      );
    wrap(<ActivityView />);
    fireEvent.click(await screen.findByRole("button", { name: "Show older" }));
    await waitFor(() => expect(list).toHaveBeenCalledWith(100));
  });

  it("reads the message out of recorded recovery evidence", () => {
    expect(recordedMessage('{"code":"A","message":"Kept both"}')).toBe(
      "Kept both",
    );
    expect(recordedMessage("Plain")).toBe("Plain");
    expect(recordedMessage(null)).toBeNull();
  });
});

describe("startup recovery notice", () => {
  it("tells the user what happened to an interrupted change, until dismissed", async () => {
    vi.spyOn(api, "getStartupRecoveryReport").mockResolvedValue([
      {
        operation_id: "x",
        title: "Install Big Sounds 1.0.0",
        outcome: "finished",
        message:
          "The app stopped while this was being done. It was finished when the app started again.",
      },
    ]);
    const dismiss = vi
      .spyOn(api, "dismissStartupRecoveryReport")
      .mockResolvedValue();
    wrap(<StartupRecoveryNotice />);
    expect(
      await screen.findByText("Finished: Install Big Sounds 1.0.0."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(dismiss).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.queryByText(/Install Big Sounds/)).toBeNull(),
    );
  });
});
