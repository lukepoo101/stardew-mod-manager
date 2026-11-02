import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsView } from "@/features/settings/SettingsView";
import { api } from "@/shared/api/client";
import type { GameInstallationSummaryDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const PREFIX =
  "/tmp/claude-1000/-home-luke-repos-stardew-mod-manager-very-long-prefix/lanes/fr1";

function game(
  id: string,
  folder: string,
  unused: boolean,
): GameInstallationSummaryDto {
  return {
    id,
    canonical_root: `${PREFIX}/${folder}`,
    operating_system: "linux",
    storefront: "manual",
    management_mode: "managed",
    created_at: "2026-11-01T10:00:00Z",
    unused,
  };
}

describe("registered game installations", () => {
  it("shows each whole path and offers removal only for unused, inactive ones", async () => {
    const active = await api.getActiveProfileOverview();
    const games = [
      game(active.game.id, "Stardew Valley", false),
      game("tried-1", "badgames/emptymods", true),
    ];
    vi.spyOn(api, "listGameInstallations").mockResolvedValue(games);
    const remove = vi
      .spyOn(api, "removeGameInstallation")
      .mockResolvedValue(undefined);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <SettingsView />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    // Paths that share a long prefix are told apart by their ends.
    expect(
      await screen.findByText(`${PREFIX}/badgames/emptymods`),
    ).toBeInTheDocument();
    expect(screen.getByText(`${PREFIX}/Stardew Valley`)).toBeInTheDocument();

    const buttons = screen.getAllByRole("button", { name: /from the manager/ });
    expect(buttons).toHaveLength(1);
    expect(buttons[0]).toHaveAccessibleName(
      `Remove ${PREFIX}/badgames/emptymods from the manager`,
    );
    const row = buttons[0].closest("div");
    expect(
      within(row as HTMLElement).queryByText("Active"),
    ).not.toBeInTheDocument();

    await act(async () => fireEvent.click(buttons[0]));
    expect(remove).toHaveBeenCalledWith("tried-1", undefined);
  });

  it("removes a mistaken active entry and makes the used one active again", async () => {
    const active = await api.getActiveProfileOverview();
    vi.spyOn(api, "listGameInstallations").mockResolvedValue([
      game("in-use", "Stardew Valley", false),
      // Registering it here made it the active game.
      game(active.game.id, "badgames/emptymods", true),
    ]);
    const remove = vi
      .spyOn(api, "removeGameInstallation")
      .mockResolvedValue(undefined);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <SettingsView />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    const button = await screen.findByRole("button", {
      name: `Remove ${PREFIX}/badgames/emptymods from the manager`,
    });
    await act(async () => fireEvent.click(button));
    expect(remove).toHaveBeenCalledWith(active.game.id, "in-use");
  });
});
