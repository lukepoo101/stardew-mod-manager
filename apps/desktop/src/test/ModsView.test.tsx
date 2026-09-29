import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ModsView } from "@/features/mods/ModsView";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const overview = {
  profile: { id: "p1", name: "Default", revision: 1, mod_count: 2 },
  game: { operating_system: "Linux", storefront: "Steam" },
  mod_count: 2,
  smapi_status: {
    is_installed: true,
    observed_version: "4.1.10",
    tested_version: "4.1.10",
    is_compatible: true,
  },
} as unknown as ProfileOverviewDto;

const mod = (over: Partial<ModListItemDto>): ModListItemDto =>
  ({
    profile_component_id: "c",
    unique_id: "A.Mod",
    name: "A Mod",
    author: "me",
    version: "1.0.0",
    description: null,
    enabled: true,
    installed_reason: "explicit",
    deployment_id: "d",
    artifact_hash: "h",
    installed_at: "",
    ...over,
  }) as ModListItemDto;

function renderMods(mods: ModListItemDto[]) {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
  vi.spyOn(api, "listProfileMods").mockResolvedValue(mods);
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <ModsView />
    </QueryClientProvider>,
  );
}

describe("mod inventory affordances", () => {
  it("copies exactly the canonical UniqueID", async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: write },
      configurable: true,
    });
    renderMods([mod({ profile_component_id: "c1", unique_id: "Author.Real" })]);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Copy UniqueID Author.Real",
      }),
    );
    await waitFor(() => expect(write).toHaveBeenCalledWith("Author.Real"));
    expect(await screen.findByText("Copied Author.Real")).toBeInTheDocument();
  });

  it("shows a missing UniqueID as unavailable instead of guessing one", async () => {
    renderMods([mod({ unique_id: "", name: "Broken" })]);
    expect(await screen.findByText(/UniqueID unavailable/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Copy UniqueID/ })).toBeNull();
  });

  it("states the third-party code trust boundary without claiming safety", async () => {
    renderMods([]);
    expect(
      await screen.findByText(/run with your user account's permissions/),
    ).toBeInTheDocument();
  });
});
