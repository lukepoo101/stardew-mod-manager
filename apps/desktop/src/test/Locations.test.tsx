import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LocationsCard } from "@/features/settings/LocationsCard";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("where things are", () => {
  it("lists each place with what it holds, and opens one by id", async () => {
    vi.spyOn(api, "getLocations").mockResolvedValue([
      {
        id: "game",
        label: "Game folder",
        path: "/games/Stardew Valley",
        exists: true,
        note: "Your Stardew Valley installation.",
      },
      {
        id: "cache",
        label: "Cache",
        path: "/home/me/.cache/smm",
        exists: false,
        note: "Safe to clear.",
      },
      {
        id: "profile_mods",
        label: "This profile's Mods folder",
        path: null,
        exists: false,
        note: "The mods SMAPI loads.",
      },
    ]);
    const reveal = vi.spyOn(api, "revealLocation").mockResolvedValue();
    render(
      <QueryClientProvider client={new QueryClient()}>
        <LocationsCard />
      </QueryClientProvider>,
    );
    fireEvent.click(screen.getByText(/Show the folders/));
    expect(
      await screen.findByText("/games/Stardew Valley"),
    ).toBeInTheDocument();
    expect(screen.getByText("Safe to clear.")).toBeInTheDocument();
    expect(screen.getByText("(not there yet)")).toBeInTheDocument();
    expect(screen.getByText("not set up")).toBeInTheDocument();
    // Only existing folders can be opened, by id.
    const open = screen.getAllByRole("button", { name: "Open" });
    expect(open).toHaveLength(1);
    fireEvent.click(open[0]);
    await waitFor(() => expect(reveal).toHaveBeenCalledWith("game"));
  });
});
