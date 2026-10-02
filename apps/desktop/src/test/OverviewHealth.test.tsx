import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OverviewView } from "@/features/overview/OverviewView";
import { api } from "@/shared/api/client";
import type { ProfileOverviewDto } from "@/shared/api/generated";
import { queryKeys } from "@/shared/api/hooks";

afterEach(() => vi.restoreAllMocks());

const overview = {
  profile: { id: "p", name: "Main", revision: 1 },
  smapi_status: { is_installed: true },
  game: { id: "g", operating_system: "linux" },
  health_summary: {
    status: "warning",
    warning_count: 1,
    error_count: 0,
    info_count: 0,
    findings: [
      {
        id: "1",
        fingerprint: "f",
        code: "X",
        severity: "warning",
        category: "runtime",
        title: "t",
        summary: "Something to look at",
        affected_entities: [],
        evidence: [],
        observed_at: "2026-10-01T10:00:00Z",
      },
    ],
  },
} as unknown as ProfileOverviewDto;

describe("overview health summary", () => {
  it("says when the checks could not be refreshed instead of showing old results as current", async () => {
    const load = vi
      .spyOn(api, "getActiveProfileOverview")
      .mockResolvedValueOnce(overview)
      .mockRejectedValue(new Error("down"));
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <OverviewView />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Something to look at")).toBeInTheDocument();
    void client.refetchQueries({ queryKey: queryKeys.overview() });
    expect(
      await screen.findByText(/Could not refresh the checks/),
    ).toBeInTheDocument();
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    expect(screen.getByText("Something to look at")).toBeInTheDocument();
  });
});
