import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { GlobalSearch } from "@/features/search/GlobalSearch";
import { rankEntries, type SearchEntry } from "@/shared/search/search";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const entry = (
  over: Partial<SearchEntry> & { title: string },
): SearchEntry => ({
  id: over.title,
  kind: "mod",
  to: "/x",
  ...over,
});

describe("ranking", () => {
  const entries = [
    entry({ title: "Content Patcher", subtitle: "Pathoschild.ContentPatcher" }),
    entry({ title: "Patch Notes", kind: "page" }),
    entry({ title: "Better Farm", keywords: ["patch"] }),
    entry({ title: "Unrelated" }),
  ];

  it("puts exact and prefix matches first and drops non-matches", () => {
    expect(rankEntries(entries, "patch").map((e) => e.title)).toEqual([
      "Patch Notes",
      "Content Patcher",
      "Better Farm",
    ]);
  });

  it("needs every word to match and ignores case and accents", () => {
    expect(rankEntries(entries, "CONTENT patcher")).toHaveLength(1);
    expect(rankEntries(entries, "content zzz")).toHaveLength(0);
    expect(rankEntries([entry({ title: "Café" })], "cafe")).toHaveLength(1);
  });

  it("lists pages and settings for an empty query", () => {
    const listed = rankEntries(
      [entry({ title: "A mod" }), entry({ title: "Mods", kind: "page" })],
      "  ",
    );
    expect(listed.map((e) => e.title)).toEqual(["Mods"]);
  });
});

function Where() {
  const location = useLocation();
  return <p data-testid="where">{location.pathname + location.search}</p>;
}

function renderSearch(onClose = vi.fn(), profilesFail = false) {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p" },
  } as unknown as ProfileOverviewDto);
  vi.spyOn(api, "listProfileMods").mockResolvedValue([
    {
      profile_component_id: "c1",
      unique_id: "Pathoschild.ContentPatcher",
      name: "Content Patcher",
      author: "Pathoschild",
    },
  ] as unknown as ModListItemDto[]);
  if (profilesFail) {
    vi.spyOn(api, "listProfiles").mockRejectedValue(new Error("db locked"));
  } else {
    vi.spyOn(api, "listProfiles").mockResolvedValue([]);
  }
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <MemoryRouter initialEntries={["/app/overview"]}>
        <GlobalSearch open onClose={onClose} />
        <Routes>
          <Route path="*" element={<Where />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return onClose;
}

describe("global search dialog", () => {
  it("finds an installed mod and opens it in the mods list with the id as the query", async () => {
    const onClose = renderSearch();
    const box = await screen.findByRole("combobox", { name: "Search" });
    fireEvent.change(box, { target: { value: "patcher" } });
    await screen.findByRole("option", { name: /Content Patcher/ });
    fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() =>
      expect(screen.getByTestId("where").textContent).toBe(
        "/app/mods?q=Pathoschild.ContentPatcher",
      ),
    );
    expect(onClose).toHaveBeenCalled();
  });

  it("moves with the arrow keys and announces the active option", async () => {
    renderSearch();
    const box = await screen.findByRole("combobox", { name: "Search" });
    const first = (await screen.findAllByRole("option"))[0];
    expect(box.getAttribute("aria-activedescendant")).toBe(first.id);
    fireEvent.keyDown(box, { key: "ArrowDown" });
    const second = screen.getAllByRole("option")[1];
    expect(box.getAttribute("aria-activedescendant")).toBe(second.id);
    expect(second).toHaveAttribute("aria-selected", "true");
  });

  it("says so when nothing matches", async () => {
    renderSearch();
    const box = await screen.findByRole("combobox", { name: "Search" });
    fireEvent.change(box, { target: { value: "qqqqqq" } });
    expect(await screen.findByText(/Nothing matches/)).toBeInTheDocument();
  });

  it("says which sources could not be searched", async () => {
    renderSearch(vi.fn(), true);
    const box = await screen.findByRole("combobox", { name: "Search" });
    fireEvent.change(box, { target: { value: "qqqqqq" } });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "profiles could not be loaded, so they were not searched",
    );
  });
});
