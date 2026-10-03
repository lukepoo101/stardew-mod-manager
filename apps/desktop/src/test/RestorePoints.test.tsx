import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RestorePointsCard } from "@/features/profiles/RestorePointsCard";
import { api } from "@/shared/api/client";
import type { ProfileOverviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

function renderCard() {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p1" },
  } as unknown as ProfileOverviewDto);
  vi.spyOn(api, "listRestorePoints").mockResolvedValue([
    {
      id: "rp1",
      label: "Working",
      created_at: "2026-09-01T10:00:00Z",
      mods: [],
      operations: ["op-remove", "op-install"],
      settings: [],
    },
  ]);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RestorePointsCard />
    </QueryClientProvider>,
  );
}

describe("restore points", () => {
  it("recreates a point as a new profile after saying what it holds", async () => {
    vi.spyOn(window, "prompt").mockReturnValue("Rebuilt");
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    vi.spyOn(api, "storedPackages").mockResolvedValue([]);
    const recreate = vi
      .spyOn(api, "recreateProfileFromPoint")
      .mockResolvedValue({
        profile_id: "p2",
        profile_name: "Rebuilt",
        installed: [],
        disabled: [],
        failures: [],
        settings_applied: [],
        declined_optional: [],
        reference_attached: false,
      });
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Recreate as new profile" }),
    );
    await waitFor(() =>
      expect(recreate).toHaveBeenCalledWith("p1", "rp1", "Rebuilt"),
    );
    expect(confirm.mock.calls[0][0]).toMatch(/This profile is not changed/);
    expect(await screen.findByText(/Created "Rebuilt"/)).toBeInTheDocument();
  });

  it("offers the last working setup as a full restore", async () => {
    vi.spyOn(api, "getKnownGood").mockResolvedValue({
      profile_id: "p1",
      recorded_at: "2026-09-01T10:00:00Z",
      game_version: null,
      smapi_version: null,
      mods: [],
      findings: null,
    });
    const plan = vi.spyOn(api, "planRestore").mockResolvedValue({
      point_id: "known-good",
      available: true,
      unavailable: [],
      remove: [],
      install: [],
      change_version: ["K.Mod 2.0.0 → 1.0.0"],
      enable: [],
      disable: [],
      settings: [],
      settings_unavailable: [],
    });
    renderCard();
    expect(
      await screen.findByText("The last working setup"),
    ).toBeInTheDocument();
    const reviews = screen.getAllByRole("button", { name: "Review restore" });
    fireEvent.click(reviews[0]);
    await waitFor(() => expect(plan).toHaveBeenCalledWith("p1", "known-good"));
    expect(await screen.findByText("K.Mod 2.0.0 → 1.0.0")).toBeInTheDocument();
  });

  it("lists settings it puts back, and saved settings that are gone", async () => {
    vi.spyOn(api, "planRestore").mockResolvedValue({
      point_id: "a1",
      available: true,
      unavailable: [],
      remove: [],
      install: [],
      change_version: [],
      enable: [],
      disable: [],
      settings: ["Speedy"],
      settings_unavailable: ["Old Mod"],
    });
    renderCard();
    const reviews = await screen.findAllByRole("button", {
      name: "Review restore",
    });
    fireEvent.click(reviews.at(-1) as HTMLElement);
    expect(await screen.findByText("Speedy")).toBeInTheDocument();
    expect(screen.getByText(/Old Mod are no longer kept/)).toBeInTheDocument();
    expect(
      screen.queryByText("The profile already matches this restore point."),
    ).toBeNull();
  });

  it("says which change an automatic point was taken before", async () => {
    renderCard();
    expect(
      await screen.findByText(/saved before 2 change\(s\) shown in Activity/),
    ).toBeInTheDocument();
  });

  it("shows the plan before restoring and restores on confirmation", async () => {
    vi.spyOn(api, "planRestore").mockResolvedValue({
      point_id: "rp1",
      available: true,
      unavailable: [],
      remove: ["Y.New 1.0.0"],
      install: [],
      change_version: ["V.Mod 2.0.0 → 1.0.0"],
      enable: [],
      disable: [],
      settings: [],
      settings_unavailable: [],
    });
    const restore = vi.spyOn(api, "restoreToPoint").mockResolvedValue({
      undo_point_id: "rp2",
      done: [],
      failed: [],
    });
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Review restore" }),
    );
    expect(await screen.findByText("Y.New 1.0.0")).toBeInTheDocument();
    expect(screen.getByText("V.Mod 2.0.0 → 1.0.0")).toBeInTheDocument();
    expect(restore).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(restore).toHaveBeenCalledWith("p1", "rp1"));
    expect(await screen.findByText('Restored "Working".')).toBeInTheDocument();
  });

  it("offers no restore when a needed package is gone", async () => {
    vi.spyOn(api, "planRestore").mockResolvedValue({
      point_id: "rp1",
      available: false,
      unavailable: ["G.Mod 1.0.0: its package is no longer kept intact"],
      remove: [],
      install: [],
      change_version: [],
      enable: [],
      disable: [],
      settings: [],
      settings_unavailable: [],
    });
    renderCard();
    fireEvent.click(
      await screen.findByRole("button", { name: "Review restore" }),
    );
    expect(
      await screen.findByText(/its package is no longer kept intact/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restore" })).toBeNull();
  });
});

describe("restoring mods and a save together", () => {
  const plan = {
    point_id: "rp1",
    available: true,
    unavailable: [],
    remove: [],
    install: [],
    change_version: ["K.Mod 2.0.0 → 1.0.0"],
    enable: [],
    disable: [],
    settings: [],
    settings_unavailable: [],
  };
  const saves = {
    saves_dir: "/saves",
    unavailable_links: [],
    saves: [
      {
        id: "Farm_1",
        farm_name: "Sunny",
        farmer_name: "Ann",
        game_version: null,
        modified_at: null,
        size_bytes: 1,
        profile_id: "p1",
        profile_name: "Main",
        backups: [
          {
            id: "b1",
            save_id: "Farm_1",
            created_at: "2026-09-01T09:00:00Z",
            size_bytes: 1,
            note: null,
          },
        ],
      },
    ],
  };

  it("restores the mods first, then the chosen save, as separate steps", async () => {
    vi.spyOn(api, "planRestore").mockResolvedValue(plan);
    vi.spyOn(api, "listSaves").mockResolvedValue(saves);
    const order: string[] = [];
    vi.spyOn(api, "restoreToPoint").mockImplementation(async () => {
      order.push("mods");
      return { undo_point_id: "u", done: [], failed: [] };
    });
    vi.spyOn(api, "restoreSaveBackup").mockImplementation(async () => {
      order.push("save");
      return {} as never;
    });
    renderCard();
    fireEvent.click(
      (await screen.findAllByRole("button", { name: "Review restore" })).at(
        -1,
      ) as HTMLElement,
    );
    fireEvent.change(await screen.findByLabelText(/Also put a save back/), {
      target: { value: "b1" },
    });
    expect(screen.getByText(/does not touch any save/)).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Restore mods, then the save" }),
    );
    await waitFor(() => expect(order).toEqual(["mods", "save"]));
  });

  it("leaves the save alone when the mods are not fully restored", async () => {
    vi.spyOn(api, "planRestore").mockResolvedValue(plan);
    vi.spyOn(api, "listSaves").mockResolvedValue(saves);
    vi.spyOn(api, "restoreToPoint").mockResolvedValue({
      undo_point_id: "u",
      done: [],
      failed: ["K.Mod: locked"],
    });
    const save = vi.spyOn(api, "restoreSaveBackup");
    renderCard();
    fireEvent.click(
      (await screen.findAllByRole("button", { name: "Review restore" })).at(
        -1,
      ) as HTMLElement,
    );
    fireEvent.change(await screen.findByLabelText(/Also put a save back/), {
      target: { value: "b1" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Restore mods, then the save" }),
    );
    expect(
      await screen.findByText(/The save was not put back/),
    ).toBeInTheDocument();
    expect(save).not.toHaveBeenCalled();
  });
});
