import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnownGoodCard } from "@/features/profiles/KnownGoodCard";
import { api } from "@/shared/api/client";
import { knownGoodDiff, restorePlan } from "@/shared/profiles/knownGood";
import type {
  FrozenModDto,
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const then = (id: string, version = "1.0", enabled = true): FrozenModDto => ({
  unique_id: id,
  name: id,
  version,
  artifact_hash: "h",
  enabled,
});
const now = (id: string, version = "1.0", enabled = true) =>
  ({
    profile_component_id: `c-${id}`,
    unique_id: id,
    name: id,
    version,
    enabled,
  }) as ModListItemDto;

describe("last known good", () => {
  const diff = knownGoodDiff(
    [then("Keep"), then("Off"), then("Upgraded", "1.0"), then("Gone")],
    [now("keep"), now("Off", "1.0", false), now("Upgraded", "2.0"), now("New")],
  );

  it("separates what restore can fix from what it cannot", () => {
    expect(diff.enabledChanged.map((c) => c.mod.name)).toEqual(["Off"]);
    expect(diff.added.map((m) => m.name)).toEqual(["New"]);
    expect(diff.removed.map((m) => m.name)).toEqual(["Gone"]);
    expect(diff.versionChanged).toEqual([
      { mod: now("Upgraded", "2.0"), was: "1.0" },
    ]);
    expect(restorePlan(diff)).toEqual({
      enable: ["c-Off"],
      disable: ["c-New"],
    });
  });

  it("explains there is no record before the first confirmed session", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([]);
    vi.spyOn(api, "getKnownGood").mockResolvedValue(null);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <KnownGoodCard />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByText(/Starting the game alone does not count/),
    ).toBeInTheDocument();
  });

  it("restores the enabled state through the reviewed toggle path", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      now("Off", "1.0", false),
      now("New"),
    ]);
    vi.spyOn(api, "getKnownGood").mockResolvedValue({
      profile_id: "p1",
      recorded_at: "2026-09-01T10:00:00Z",
      game_version: "1.6.15",
      smapi_version: "4.1.10",
      mods: [then("Off")],
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const toggle = vi
      .spyOn(api, "setModsEnabled")
      .mockResolvedValue({ changed: [], failed: [] });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <KnownGoodCard />
      </QueryClientProvider>,
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Restore enabled state" }),
    );
    await waitFor(() => expect(toggle).toHaveBeenCalledWith(["c-Off"], true));
    await waitFor(() => expect(toggle).toHaveBeenCalledWith(["c-New"], false));
    expect(
      await screen.findByText("Enabled state restored."),
    ).toBeInTheDocument();
  });
});
