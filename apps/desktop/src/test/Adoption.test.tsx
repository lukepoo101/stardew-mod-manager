import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AdoptionCard } from "@/features/profiles/AdoptionCard";
import { api } from "@/shared/api/client";
import type { ProfileOverviewDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const component = (id: string) => ({
  unique_id: id,
  name: id,
  version: "1.0",
  author: "a",
  update_keys: [],
});

const scan = {
  mods_dir: "/game/Mods",
  mods: [
    {
      folder: "Alpha",
      components: [component("A.Alpha")],
      size_bytes: 2048,
      file_count: 2,
      has_settings: true,
      problems: [],
      duplicate_of: [],
      stored: "none",
    },
    {
      folder: "Beta",
      components: [component("B.Beta")],
      size_bytes: 1024,
      file_count: 2,
      has_settings: false,
      problems: [],
      duplicate_of: ["BetaCopy"],
      stored: "same_version",
    },
    {
      folder: "BetaCopy",
      components: [component("B.Beta")],
      size_bytes: 1024,
      file_count: 2,
      has_settings: false,
      problems: [],
      duplicate_of: ["Beta"],
      stored: "none",
    },
  ],
  unknown: [
    {
      name: "Stuff",
      size_bytes: 10,
      is_folder: true,
      reason: "No manifest.json",
    },
  ],
  runtime: ["ConsoleCommands"],
  manager_markers: ["Vortex (__folder_managed_by_vortex) in Alpha"],
  fingerprint: "fp",
  total_bytes: 4096,
};

const show = () => {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
    profile: { id: "p1" },
    game: { id: "g1" },
  } as unknown as ProfileOverviewDto);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AdoptionCard />
    </QueryClientProvider>,
  );
};

describe("adopting existing mods", () => {
  it("previews, leaves duplicates for the user, and adopts the chosen ones", async () => {
    vi.spyOn(api, "scanModsFolder").mockResolvedValue(scan);
    const adopt = vi.spyOn(api, "adoptMods").mockResolvedValue({
      profile_id: "new",
      profile_name: "Adopted mods",
      adopted: ["Alpha", "Beta"],
      failed: [],
      left_in_place: ["BetaCopy", "Stuff"],
    });
    show();
    fireEvent.click(
      await screen.findByRole("button", { name: "Scan the Mods folder" }),
    );
    expect(await screen.findByText(/Another mod manager/)).toBeInTheDocument();
    expect(
      screen.getByText(/Part of SMAPI, not adopted: ConsoleCommands/),
    ).toBeInTheDocument();
    // Duplicates start unticked; ticking both blocks adoption.
    const beta = screen.getByRole("checkbox", { name: /^Beta:/ });
    expect(beta).not.toBeChecked();
    fireEvent.click(beta);
    fireEvent.click(screen.getByRole("checkbox", { name: /^BetaCopy:/ }));
    expect(
      screen.getByText(/Two chosen folders hold the same mod/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: /^BetaCopy:/ }));
    fireEvent.click(
      screen.getByRole("button", { name: /Adopt 2 into a new profile/ }),
    );
    await waitFor(() =>
      expect(adopt).toHaveBeenCalledWith(
        "g1",
        "Adopted mods",
        ["Alpha", "Beta"],
        "fp",
      ),
    );
    expect(
      await screen.findByText(/The Mods folder was not changed/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Left as they were: BetaCopy, Stuff/),
    ).toBeInTheDocument();
  });

  it("copies a profile's mods to a plain folder", async () => {
    vi.spyOn(api, "pickFolderDialog").mockResolvedValue("/out");
    const exportMods = vi
      .spyOn(api, "exportModsFolder")
      .mockResolvedValue("/out/Mods");
    show();
    fireEvent.click(
      await screen.findByRole("button", {
        name: /Copy this profile's mods to a folder/,
      }),
    );
    await waitFor(() => expect(exportMods).toHaveBeenCalledWith("p1", "/out"));
    expect(
      await screen.findByText(/Copied to \/out\/Mods/),
    ).toBeInTheDocument();
  });
});
