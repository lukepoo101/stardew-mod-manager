import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ModsView } from "@/features/mods/ModsView";
import { api } from "@/shared/api/client";
import {
  allTags,
  indexAnnotations,
  parseTagInput,
  sortMods,
} from "@/shared/mods/organise";
import type {
  ModAnnotationDto,
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

const mod = (
  id: string,
  name: string,
  over: Partial<ModListItemDto> = {},
): ModListItemDto =>
  ({
    profile_component_id: `c-${id}`,
    unique_id: id,
    name,
    author: "Author",
    version: "1.0.0",
    description: null,
    enabled: true,
    installed_reason: "explicit",
    deployment_id: "d",
    artifact_hash: "h",
    installed_at: "2026-01-01T00:00:00Z",
    ...over,
  }) as ModListItemDto;

const note = (
  unique_id: string,
  over: Partial<ModAnnotationDto> = {},
): ModAnnotationDto => ({
  unique_id,
  favourite: false,
  tags: [],
  note: "",
  ...over,
});

describe("mod organisation logic", () => {
  const mods = [
    mod("B.Mod", "beta", {
      author: "Zed",
      installed_at: "2026-03-01T00:00:00Z",
    }),
    mod("A.Mod", "Alpha", { enabled: false, author: "Amy" }),
    mod("C.Mod", "gamma", { installed_at: "2026-02-01T00:00:00Z" }),
  ];
  const index = indexAnnotations([
    note("c.mod", { favourite: true, tags: ["UI", "visuals"] }),
    note("B.Mod", { tags: ["ui"] }),
  ]);
  const names = (list: ModListItemDto[]) => list.map((m) => m.name);

  it("sorts by each key with a stable name tiebreak", () => {
    expect(names(sortMods(mods, "name", index))).toEqual([
      "Alpha",
      "beta",
      "gamma",
    ]);
    expect(names(sortMods(mods, "author", index))).toEqual([
      "Alpha",
      "gamma",
      "beta",
    ]);
    expect(names(sortMods(mods, "newest", index))).toEqual([
      "beta",
      "gamma",
      "Alpha",
    ]);
    expect(names(sortMods(mods, "favourites", index))[0]).toBe("gamma");
    expect(names(sortMods(mods, "enabled", index)).at(-1)).toBe("Alpha");
  });

  it("lists tags once regardless of case and parses tag input", () => {
    expect(allTags(index)).toEqual(["UI", "visuals"]);
    expect(parseTagInput(" visuals,  needs   config , Visuals,,")).toEqual([
      "visuals",
      "needs config",
    ]);
  });
});

describe("mod organisation in the Mods page", () => {
  const overview = {
    profile: { id: "p1", name: "Default", revision: 1, mod_count: 2 },
    game: { operating_system: "Linux", storefront: "Steam" },
    mod_count: 2,
    smapi_status: { is_installed: true, is_compatible: true },
  } as unknown as ProfileOverviewDto;

  function renderWith(annotations: ModAnnotationDto[]) {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      mod("A.Mod", "Alpha"),
      mod("B.Mod", "Beta"),
    ]);
    vi.spyOn(api, "listModAnnotations").mockResolvedValue(annotations);
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

  it("marks a favourite without touching any other annotation", async () => {
    const set = vi
      .spyOn(api, "setModAnnotation")
      .mockImplementation(async (a) => a);
    renderWith([note("B.Mod", { tags: ["keep"], note: "mine" })]);
    fireEvent.click(
      await screen.findByRole("button", { name: "Favourite Beta" }),
    );
    await waitFor(() =>
      expect(set).toHaveBeenCalledWith({
        unique_id: "B.Mod",
        favourite: true,
        tags: ["keep"],
        note: "mine",
      }),
    );
  });

  it("filters by tag and shows tags on each row", async () => {
    renderWith([note("a.mod", { tags: ["visuals"] })]);
    expect(await screen.findByText("Beta")).toBeInTheDocument();
    const tags = screen.getByRole("list", { name: "Tags" });
    expect(within(tags).getByText("visuals")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Tag"), {
      target: { value: "visuals" },
    });
    await waitFor(() => expect(screen.queryByText("Beta")).toBeNull());
    expect(screen.getByText("Alpha")).toBeInTheDocument();
  });
});

describe("sort direction, version order and favourites", () => {
  const mods = [
    mod("A.Mod", "Alpha", { version: "1.10.0" }),
    mod("B.Mod", "Beta", { version: "1.2.0" }),
    mod("C.Mod", "Gamma", { version: "1.9.0" }),
  ];
  const index = indexAnnotations([]);
  const names = (list: ModListItemDto[]) => list.map((m) => m.name);

  it("orders versions numerically and can reverse any sort", () => {
    expect(names(sortMods(mods, "version", index))).toEqual([
      "Beta",
      "Gamma",
      "Alpha",
    ]);
    expect(names(sortMods(mods, "version", index, true))).toEqual([
      "Alpha",
      "Gamma",
      "Beta",
    ]);
    expect(names(sortMods(mods, "name", index, true))).toEqual([
      "Gamma",
      "Beta",
      "Alpha",
    ]);
  });
});

describe("favourites filter", () => {
  it("shows only favourites when asked", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Default", revision: 1, mod_count: 2 },
      game: { operating_system: "Linux", storefront: "Steam" },
      mod_count: 2,
      smapi_status: { is_installed: true, is_compatible: true },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      mod("A.Mod", "Alpha"),
      mod("B.Mod", "Beta"),
    ]);
    vi.spyOn(api, "listModAnnotations").mockResolvedValue([
      note("B.Mod", { favourite: true }),
    ]);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ModsView />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Favourites only"));
    await waitFor(() => expect(screen.queryByText("Alpha")).toBeNull());
    expect(screen.getByText("Beta")).toBeInTheDocument();
  });
});
