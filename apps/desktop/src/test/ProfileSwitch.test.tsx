import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProfilesView } from "@/features/profiles/ProfilesView";
import { api } from "@/shared/api/client";
import type {
  ProfileOverviewDto,
  ProfileSummaryDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

describe("switching profiles", () => {
  it("names both profiles and says the old one stays active when it fails", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "a", name: "Main", revision: 1 },
      game: { id: "g" },
      smapi_status: { is_installed: true, observed_version: "4.1.10" },
      health_summary: {
        status: "healthy",
        warning_count: 0,
        error_count: 0,
        info_count: 0,
        findings: [],
      },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfiles").mockResolvedValue([
      {
        id: "a",
        name: "Main",
        mod_count: 0,
        revision: 1,
        created_at: "",
        updated_at: "",
      },
      {
        id: "b",
        name: "Expanded",
        mod_count: 0,
        revision: 1,
        created_at: "",
        updated_at: "",
      },
    ] as unknown as ProfileSummaryDto[]);
    vi.spyOn(api, "listArchivedProfiles").mockResolvedValue([]);
    vi.spyOn(api, "listSaves").mockResolvedValue({
      saves_dir: "",
      saves: [],
      unavailable_links: [],
    });
    vi.spyOn(api, "listDeletedProfiles").mockResolvedValue([]);
    vi.spyOn(api, "activateProfile").mockRejectedValue(
      Object.assign(new Error("Stop the game first"), {
        code: "GAME_RUNNING",
      }),
    );
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <ProfilesView />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Switch from Main to Expanded",
      }),
    );
    expect(
      await screen.findByText(/"Main" is still active/),
    ).toBeInTheDocument();
  });
});
