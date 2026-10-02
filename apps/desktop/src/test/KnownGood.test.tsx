import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnownGoodCard } from "@/features/profiles/KnownGoodCard";
import { api } from "@/shared/api/client";
import {
  healthChanges,
  knownGoodDiff,
  restorePlan,
} from "@/shared/profiles/knownGood";
import type {
  FindingDto,
  FrozenModDto,
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const renderCard = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <KnownGoodCard />
    </QueryClientProvider>,
  );

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
      findings: null,
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

describe("changes since the last good session", () => {
  it("lists successful operations after the record, without claiming cause", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([now("New")]);
    vi.spyOn(api, "getKnownGood").mockResolvedValue({
      profile_id: "p1",
      recorded_at: "2026-09-01T10:00:00Z",
      game_version: null,
      smapi_version: null,
      mods: [],
      findings: null,
    });
    vi.spyOn(api, "listRecentOperations").mockResolvedValue([
      {
        id: "after",
        kind: "mod_install",
        state: "succeeded",
        profile_id: "p1",
        created_at: "2026-09-02T10:00:00Z",
      },
      {
        id: "before",
        kind: "mod_remove",
        state: "succeeded",
        profile_id: "p1",
        created_at: "2026-08-01T10:00:00Z",
      },
      {
        id: "other",
        kind: "mod_install",
        state: "succeeded",
        profile_id: "p2",
        created_at: "2026-09-03T10:00:00Z",
      },
    ] as never);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <KnownGoodCard />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Changes made since")).toBeInTheDocument();
    expect(screen.getAllByText(/mod install,/)).toHaveLength(1);
    expect(screen.queryByText(/mod remove/)).toBeNull();
    expect(screen.getByText(/not proof/)).toBeInTheDocument();
  });
});

describe("health since the last good session", () => {
  const finding = (fingerprint: string, severity: string) =>
    ({
      id: fingerprint,
      fingerprint,
      code: "X",
      severity,
      category: "dependency",
      title: `Finding ${fingerprint}`,
      summary: "",
      affected_entities: [],
      evidence: [],
      observed_at: "",
    }) as FindingDto;
  const baseline = (fingerprint: string, severity: string) => ({
    fingerprint,
    code: "X",
    severity,
    title: `Finding ${fingerprint}`,
  });

  it("separates new, worse, gone and unchanged findings", () => {
    const changes = healthChanges(
      [
        baseline("same", "warning"),
        baseline("worse", "info"),
        baseline("gone", "error"),
        baseline("milder", "error"),
      ],
      [
        finding("same", "warning"),
        finding("worse", "error"),
        finding("milder", "warning"),
        finding("new", "error"),
      ],
    );
    expect(changes.introduced.map((f) => f.fingerprint)).toEqual(["new"]);
    expect(changes.escalated).toEqual([
      { finding: finding("worse", "error"), was: "info" },
    ]);
    expect(changes.resolved.map((f) => f.fingerprint)).toEqual(["gone"]);
    expect(changes.unchanged).toBe(2);
  });

  it("says when there is no health baseline to compare with", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
      health_summary: { findings: [] },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([]);
    vi.spyOn(api, "getKnownGood").mockResolvedValue({
      profile_id: "p1",
      recorded_at: "2026-09-01T10:00:00Z",
      game_version: null,
      smapi_version: null,
      mods: [],
      findings: null,
    });
    renderCard();
    expect(
      await screen.findByText(/Health then was not recorded/),
    ).toBeInTheDocument();
  });

  it("lists a finding that appeared since, without claiming cause", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
      health_summary: { findings: [finding("new", "error")] },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([]);
    vi.spyOn(api, "getKnownGood").mockResolvedValue({
      profile_id: "p1",
      recorded_at: "2026-09-01T10:00:00Z",
      game_version: null,
      smapi_version: null,
      mods: [],
      findings: [],
    });
    renderCard();
    expect(
      await screen.findByText("New: Finding new (error)"),
    ).toBeInTheDocument();
    expect(screen.getByText(/timing, not proof/)).toBeInTheDocument();
  });
});

describe("forgetting the last working setup", () => {
  it("needs the word typed and does nothing otherwise", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1" },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([now("A")]);
    vi.spyOn(api, "getKnownGood").mockResolvedValue({
      profile_id: "p1",
      recorded_at: "2026-09-01T10:00:00Z",
      game_version: null,
      smapi_version: null,
      mods: [then("A")],
      findings: null,
    });
    const forget = vi.spyOn(api, "forgetKnownGood").mockResolvedValue();
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("nope");
    render(
      <QueryClientProvider client={new QueryClient()}>
        <KnownGoodCard />
      </QueryClientProvider>,
    );
    const button = await screen.findByRole("button", {
      name: "Forget this record...",
    });
    fireEvent.click(button);
    expect(prompt.mock.calls[0][0]).toMatch(/can no longer restore it/);
    expect(forget).not.toHaveBeenCalled();
    prompt.mockReturnValue("forget");
    fireEvent.click(button);
    await waitFor(() => expect(forget).toHaveBeenCalledWith("p1"));
  });
});
