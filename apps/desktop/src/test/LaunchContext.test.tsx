import { MemoryRouter } from "react-router-dom";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ContextHeader } from "@/components/layout/ContextHeader";
import { api } from "@/shared/api/client";
import type { ProfileOverviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

describe("launch context", () => {
  it("marks an experiment and a frozen profile next to its name", async () => {
    vi.spyOn(api, "listExperiments").mockResolvedValue([
      {
        profile_id: "exp",
        source_profile_id: "src",
        source_name: "Main",
        source_revision: 1,
        created_at: "",
      },
    ]);
    vi.spyOn(api, "getProfileFreeze").mockResolvedValue({
      profile_id: "exp",
      frozen_at: "",
      reason: "Co-op",
      mods: [],
      settings: [],
      game_version: null,
      smapi_version: null,
    });
    vi.spyOn(api, "listDismissedFindings").mockResolvedValue([]);
    const overview = {
      profile: { id: "exp", name: "Main experiment", revision: 2 },
      game: { canonical_root: "/games/sdv" },
      health_summary: { findings: [] },
      smapi_status: { is_installed: true, is_compatible: true },
    } as unknown as ProfileOverviewDto;
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <ContextHeader overview={overview} />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    expect(screen.getByText("Main experiment")).toBeInTheDocument();
    expect(await screen.findByText("Experiment")).toBeInTheDocument();
    expect(await screen.findByText("Frozen")).toBeInTheDocument();
  });
});
