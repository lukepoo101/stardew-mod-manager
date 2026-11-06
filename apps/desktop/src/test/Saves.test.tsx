import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "bun:test";
import { SavesCard } from "@/features/saves/SavesCard";
import { api } from "@/shared/api/client";
import type {
  ProfileOverviewDto,
  ProfileSummaryDto,
  SavesDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const saves: SavesDto = {
  saves_dir: "/home/me/.config/StardewValley/Saves",
  unavailable_links: [],
  saves: [
    {
      id: "Riverside_1",
      farm_name: "Riverside",
      farmer_name: "Sam",
      game_version: "1.6.15",
      modified_at: "2026-09-01T10:00:00Z",
      info_unreadable: false,
      size_bytes: 100,
      profile_id: "coop",
      profile_name: "Co-op",
      backups: [
        {
          id: "Riverside_1/20260901T100000000",
          save_id: "Riverside_1",
          created_at: "2026-09-01T10:00:00Z",
          size_bytes: 100,
          note: null,
        },
      ],
    },
  ],
};

function renderCard(activeId: string) {
  vi.spyOn(api, "listSaves").mockResolvedValue(saves);
  vi.spyOn(api, "listProfiles").mockResolvedValue([
    { id: "coop", name: "Co-op" },
    { id: "solo", name: "Solo" },
  ] as ProfileSummaryDto[]);
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: activeId, name: activeId === "coop" ? "Co-op" : "Solo" },
  } as unknown as ProfileOverviewDto);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SavesCard />
    </QueryClientProvider>,
  );
}

describe("saves", () => {
  it("warns when the last played farm belongs to another profile", async () => {
    renderCard("solo");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /Riverside Farm, is linked to "Co-op", but "Solo" is active/,
    );
  });

  it("does not warn when the right profile is active", async () => {
    renderCard("coop");
    expect(await screen.findByText("Riverside Farm")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("links, backs up and restores through explicit actions", async () => {
    const link = vi.spyOn(api, "associateSave").mockResolvedValue();
    const backup = vi
      .spyOn(api, "backupSave")
      .mockResolvedValue(saves.saves[0].backups[0]);
    const restore = vi
      .spyOn(api, "restoreSaveBackup")
      .mockResolvedValue(saves.saves[0].backups[0]);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    renderCard("coop");
    fireEvent.change(await screen.findByLabelText("Usual profile"), {
      target: { value: "solo" },
    });
    await waitFor(() =>
      expect(link).toHaveBeenCalledWith("Riverside_1", "solo"),
    );
    fireEvent.click(screen.getByRole("button", { name: "Back up now" }));
    await waitFor(() => expect(backup).toHaveBeenCalledWith("Riverside_1"));
    fireEvent.click(screen.getByRole("button", { name: "Restore..." }));
    await waitFor(() =>
      expect(restore).toHaveBeenCalledWith("Riverside_1/20260901T100000000"),
    );
    expect(
      await screen.findByText(/The save it replaced was kept as a backup/),
    ).toBeInTheDocument();
  });
});

describe("save names and damaged saves", () => {
  it("does not double Farm and flags a save whose info cannot be read", async () => {
    vi.spyOn(api, "listSaves").mockResolvedValue({
      ...saves,
      saves: [
        { ...saves.saves[0], farm_name: "Ridge Farm" },
        {
          ...saves.saves[0],
          id: "Broken_2",
          farm_name: null,
          farmer_name: null,
          profile_id: null,
          profile_name: null,
          backups: [],
          info_unreadable: true,
        },
      ],
    });
    vi.spyOn(api, "listProfiles").mockResolvedValue([]);
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "coop", name: "Co-op" },
    } as unknown as ProfileOverviewDto);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SavesCard />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Ridge Farm")).toBeInTheDocument();
    expect(screen.getByText("Broken_2")).toBeInTheDocument();
    expect(
      screen.getByText(/SaveGameInfo file is empty or could not be read/),
    ).toBeInTheDocument();
  });

  it("names the profile a save was linked to", async () => {
    vi.spyOn(api, "associateSave").mockResolvedValue();
    renderCard("coop");
    fireEvent.change(await screen.findByLabelText("Usual profile"), {
      target: { value: "solo" },
    });
    expect(
      await screen.findByText('Riverside Farm is linked to "Solo".'),
    ).toBeInTheDocument();
  });
});

describe("links to saves that are gone", () => {
  it("lists them and removes a link on request", async () => {
    vi.spyOn(api, "listSaves").mockResolvedValue({
      ...saves,
      unavailable_links: [
        { save_id: "Oldfarm_9", profile_id: "solo", profile_name: "Solo" },
      ],
    });
    vi.spyOn(api, "listProfiles").mockResolvedValue([
      { id: "solo", name: "Solo" },
    ] as ProfileSummaryDto[]);
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "coop", name: "Co-op" },
    } as unknown as ProfileOverviewDto);
    const unlink = vi.spyOn(api, "associateSave").mockResolvedValue();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <SavesCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText("Oldfarm_9, linked to Solo"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove link" }));
    await waitFor(() => expect(unlink).toHaveBeenCalledWith("Oldfarm_9", null));
  });
});
