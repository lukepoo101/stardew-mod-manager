import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModsView } from "@/features/mods/ModsView";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";
import { matchesEveryWord, rankEntries } from "@/shared/search/search";

afterEach(() => vi.restoreAllMocks());

const CAFE = ["Café Menu Déluxe", "Zoe.CafeMenu", "Zoë"];
const PATCHER = [
  "Content Patcher",
  "Pathoschild.ContentPatcher",
  "Pathoschild",
];
const FILLER = (n: string) => [`Filler Mod ${n}`, `Filler.Mod${n}`, "Pw"];

describe("matching every word", () => {
  it.each([
    ["cafe deluxe", CAFE],
    ["deluxe", CAFE],
    ["DÉLUXE café", CAFE],
    ["zoe", CAFE],
    ["patcher content", PATCHER],
    ["content PATCHER", PATCHER],
    ["  content   patcher ", PATCHER],
    ["pathoschild", PATCHER],
    ["filler 2", FILLER("02")],
    ["filler 2", FILLER("12")],
    ["", PATCHER],
  ])("%j finds %j", (query, fields) => {
    expect(matchesEveryWord(query, fields)).toBe(true);
  });

  it.each([
    ["cafe patcher", CAFE],
    ["filler 2", FILLER("01")],
    ["tractor mod", PATCHER],
  ])("%j does not find %j", (query, fields) => {
    expect(matchesEveryWord(query, fields)).toBe(false);
  });

  it("agrees with global search on what matches", () => {
    const entries = [
      {
        id: "a",
        kind: "mod" as const,
        title: CAFE[0],
        subtitle: CAFE[1],
        keywords: [CAFE[2]],
        to: "/",
      },
      {
        id: "b",
        kind: "mod" as const,
        title: PATCHER[0],
        subtitle: PATCHER[1],
        keywords: [PATCHER[2]],
        to: "/",
      },
    ];
    for (const query of ["cafe deluxe", "patcher content", "zoe", "menu"]) {
      const global = rankEntries(entries, query).map((entry) => entry.id);
      const page = entries
        .filter((entry) =>
          matchesEveryWord(query, [
            entry.title,
            entry.subtitle,
            ...entry.keywords,
          ]),
        )
        .map((entry) => entry.id);
      expect(page.sort()).toEqual(global.sort());
    }
  });
});

describe("the Mods page search", () => {
  it("finds a mod by words in any order, without accents", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Main", revision: 1 },
      game: { id: "g" },
    } as unknown as ProfileOverviewDto);
    const mod = (id: string, name: string, uniqueId: string) =>
      ({
        profile_component_id: id,
        unique_id: uniqueId,
        name,
        author: "Someone",
        version: "1.0.0",
        description: null,
        enabled: true,
        installed_reason: "explicit",
        deployment_id: `d-${id}`,
        artifact_hash: "h",
        installed_at: "",
      }) as unknown as ModListItemDto;
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      mod("1", "Café Menu Déluxe", "Zoe.CafeMenu"),
      mod("2", "Content Patcher", "Pathoschild.ContentPatcher"),
    ]);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ModsView />
      </QueryClientProvider>,
    );
    const box = await screen.findByRole("searchbox", { name: "Search mods" });
    await screen.findByText("Content Patcher");

    fireEvent.change(box, { target: { value: "deluxe cafe" } });
    expect(screen.getByText("Café Menu Déluxe")).toBeInTheDocument();
    expect(screen.queryByText("Content Patcher")).not.toBeInTheDocument();

    fireEvent.change(box, { target: { value: "patcher content" } });
    expect(screen.getByText("Content Patcher")).toBeInTheDocument();
    expect(screen.queryByText("Café Menu Déluxe")).not.toBeInTheDocument();

    // With a filter on as well, the empty state says the filter hides it.
    fireEvent.click(screen.getByRole("button", { name: /^Disabled/ }));
    expect(
      screen.getByText('No mods match "patcher content" with these filters'),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "2 mod(s) are in this profile; 1 match the search, and the filters hide them.",
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Show all mods" }));
    expect(screen.getByText("Content Patcher")).toBeInTheDocument();
    expect(screen.getByText("Café Menu Déluxe")).toBeInTheDocument();
  });
});
