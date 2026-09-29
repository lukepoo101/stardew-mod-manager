import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { ModsView } from "@/features/mods/ModsView";
import { ActivityView } from "@/features/activity/ActivityView";
import { EmptyState } from "@/components/ui/EmptyState";
import { api } from "@/shared/api/client";
import { savePreferences, DEFAULT_PREFERENCES } from "@/shared/preferences";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

const overview = {
  profile: { id: "p1", name: "Default", revision: 1, mod_count: 1 },
  game: { operating_system: "Linux", storefront: "Steam" },
  mod_count: 1,
  smapi_status: { is_installed: true, is_compatible: true },
} as unknown as ProfileOverviewDto;

const disabledMod = {
  profile_component_id: "c",
  unique_id: "A.Mod",
  name: "A Mod",
  author: "me",
  version: "1.0.0",
  description: null,
  enabled: false,
  installed_reason: "explicit",
} as unknown as ModListItemDto;

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

describe("empty states", () => {
  it("offers the local install path when a profile has no mods", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([]);
    wrap(<ModsView />);
    expect(
      await screen.findByText("No mods in this profile yet"),
    ).toBeInTheDocument();
    expect(screen.getByText(/No account is needed/)).toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: "Choose mod ZIP" }).length,
    ).toBeGreaterThan(1);
  });

  it("says a filter hides mods rather than claiming there are none", async () => {
    savePreferences({ ...DEFAULT_PREFERENCES, modFilter: "enabled" });
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([disabledMod]);
    wrap(<ModsView />);
    expect(
      await screen.findByText("No enabled mods match"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Show all mods" }));
    expect(await screen.findByText("A Mod")).toBeInTheDocument();
  });

  it("shows a load failure instead of an empty list", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockRejectedValue(new Error("disk gone"));
    wrap(<ModsView />);
    expect(
      await screen.findByText("Could not load this profile's mods"),
    ).toBeInTheDocument();
    expect(screen.queryByText("No mods in this profile yet")).toBeNull();
  });

  it("points an empty activity log at installing a mod", async () => {
    vi.spyOn(api, "listRecentOperations").mockResolvedValue([]);
    wrap(<ActivityView />);
    expect(
      await screen.findByText("Nothing has happened yet"),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Install a mod" })).toHaveAttribute(
      "href",
      "/app/mods",
    );
  });

  it("hides guidance text but keeps the action when guidance is off", () => {
    savePreferences({ ...DEFAULT_PREFERENCES, showGuidance: false });
    render(
      <EmptyState
        title="Empty"
        description="Long explanation"
        actions={<button type="button">Do it</button>}
      />,
    );
    expect(screen.queryByText("Long explanation")).toBeNull();
    expect(screen.getByRole("button", { name: "Do it" })).toBeInTheDocument();
  });
});
