import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ExperimentCard } from "@/features/profiles/ExperimentCard";
import { api } from "@/shared/api/client";
import type {
  ProfileOverviewDto,
  ProfileSummaryDto,
  SavesDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

function renderCard(activeId: string, activeName: string) {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: activeId, name: activeName, mod_count: 4 },
  } as unknown as ProfileOverviewDto);
  vi.spyOn(api, "listProfiles").mockResolvedValue([
    { id: "src", name: "Main" },
    { id: "exp", name: "Main experiment" },
  ] as ProfileSummaryDto[]);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ExperimentCard />
    </QueryClientProvider>,
  );
}

const saves = {
  saves_dir: "/saves",
  unavailable_links: [],
  saves: [
    {
      id: "Farm_1",
      farm_name: "Sunny",
      farmer_name: "Ash",
      game_version: "1.6.15",
      modified_at: null,
      size_bytes: 10,
      profile_id: null,
      profile_name: null,
      backups: [],
    },
  ],
} as SavesDto;

const started = {
  settings_applied: [],
  declined_optional: [],
  reference_attached: false,
  profile_id: "exp",
  profile_name: "Main experiment",
  installed: [],
  disabled: [],
  failures: [],
};

describe("experiments", () => {
  it("backs up a chosen save before starting", async () => {
    vi.spyOn(api, "listExperiments").mockResolvedValue([]);
    vi.spyOn(api, "listSaves").mockResolvedValue(saves);
    const backup = vi.spyOn(api, "backupSave").mockResolvedValue({
      id: "b1",
      save_id: "Farm_1",
      created_at: "2026-10-01T10:00:00Z",
      size_bytes: 10,
    });
    const start = vi.spyOn(api, "startExperiment").mockResolvedValue(started);
    vi.spyOn(api, "activateProfile").mockResolvedValue();
    renderCard("src", "Main");
    fireEvent.change(await screen.findByLabelText("Back up a save first:"), {
      target: { value: "Farm_1" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Start an experiment" }),
    );
    await waitFor(() => expect(start).toHaveBeenCalled());
    expect(backup).toHaveBeenCalledWith("Farm_1");
    expect(backup.mock.invocationCallOrder[0]).toBeLessThan(
      start.mock.invocationCallOrder[0],
    );
    expect(
      await screen.findByText(/Sunny \(Farm_1\) was backed up/),
    ).toBeInTheDocument();
  });

  it("does not start when the requested save backup fails", async () => {
    vi.spyOn(api, "listExperiments").mockResolvedValue([]);
    vi.spyOn(api, "listSaves").mockResolvedValue(saves);
    vi.spyOn(api, "backupSave").mockRejectedValue(new Error("disk full"));
    const start = vi.spyOn(api, "startExperiment").mockResolvedValue(started);
    renderCard("src", "Main");
    fireEvent.change(await screen.findByLabelText("Back up a save first:"), {
      target: { value: "Farm_1" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Start an experiment" }),
    );
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(start).not.toHaveBeenCalled();
  });

  it("copies the active profile and switches to the copy", async () => {
    vi.spyOn(api, "listExperiments").mockResolvedValue([]);
    const start = vi.spyOn(api, "startExperiment").mockResolvedValue({
      settings_applied: [],
      declined_optional: [],
      reference_attached: false,
      profile_id: "exp",
      profile_name: "Main experiment",
      installed: [],
      disabled: [],
      failures: [],
    });
    const activate = vi.spyOn(api, "activateProfile").mockResolvedValue();
    renderCard("src", "Main");
    fireEvent.click(
      await screen.findByRole("button", { name: "Start an experiment" }),
    );
    await waitFor(() =>
      expect(start).toHaveBeenCalledWith("src", "Main experiment"),
    );
    await waitFor(() => expect(activate).toHaveBeenCalledWith("exp"));
    expect(await screen.findByText(/"Main" is unchanged/)).toBeInTheDocument();
  });

  it("discards by switching back, then archiving and deleting the copy", async () => {
    vi.spyOn(api, "listExperiments").mockResolvedValue([
      {
        profile_id: "exp",
        source_profile_id: "src",
        source_name: "Main",
        source_revision: 3,
        created_at: "2026-09-01T10:00:00Z",
      },
    ]);
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const calls: string[] = [];
    vi.spyOn(api, "activateProfile").mockImplementation(async (id) => {
      calls.push(`activate ${id}`);
    });
    vi.spyOn(api, "archiveProfile").mockImplementation(async (id) => {
      calls.push(`archive ${id}`);
    });
    vi.spyOn(api, "deleteProfile").mockImplementation(async (id) => {
      calls.push(`delete ${id}`);
    });
    vi.spyOn(api, "keepExperiment").mockImplementation(async (id) => {
      calls.push(`forget ${id}`);
    });
    renderCard("exp", "Main experiment");
    fireEvent.click(
      await screen.findByRole("button", {
        name: 'Discard and go back to "Main"',
      }),
    );
    await waitFor(() =>
      expect(calls).toEqual([
        "activate src",
        "archive exp",
        "delete exp",
        "forget exp",
      ]),
    );
  });
});
