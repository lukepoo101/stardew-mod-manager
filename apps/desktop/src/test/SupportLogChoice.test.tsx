import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SupportExportCard } from "@/features/diagnostics/SupportExportCard";
import { api } from "@/shared/api/client";
import type { LaunchSessionDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

describe("choosing the log for a support export", () => {
  it("uses a session's saved log, and says when there is none", async () => {
    vi.spyOn(api, "listLaunchSessions").mockResolvedValue([
      {
        id: "s-old",
        launched_at: "2026-10-01T09:00:00Z",
      } as unknown as LaunchSessionDto,
    ]);
    const base = await api.getDiagnosticsReport();
    const fetch = vi.spyOn(api, "getDiagnosticsReport").mockResolvedValue({
      ...base,
      raw_log: "[09:00:00 ERROR Old] the old problem",
      log_is_saved_copy: true,
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SupportExportCard report={base} overview={undefined} mods={[]} />
      </QueryClientProvider>,
    );
    fireEvent.change(await screen.findByLabelText("Log to include"), {
      target: { value: "s-old" },
    });
    expect(await screen.findByText(/the old problem/)).toBeInTheDocument();
    expect(fetch).toHaveBeenCalledWith(undefined, "s-old");
    expect(
      screen.getAllByText(/from the saved log of the session started/).length,
    ).toBeGreaterThan(0);
  });
});
