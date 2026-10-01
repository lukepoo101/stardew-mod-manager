import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProfileCompareCard } from "@/features/profiles/ProfileCompareCard";
import { api } from "@/shared/api/client";
import type { ProfileSummaryDto } from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

describe("comparing two profiles' settings", () => {
  it("lists which mods' settings differ without their contents", async () => {
    vi.spyOn(api, "listProfiles").mockResolvedValue([
      { id: "a", name: "Solo" },
      { id: "b", name: "Co-op" },
    ] as ProfileSummaryDto[]);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([]);
    const compare = vi.spyOn(api, "compareProfileSettings").mockResolvedValue([
      {
        unique_id: "a.mod",
        name: "A Mod",
        state: "different",
        files: ["config.json"],
      },
      { unique_id: "b.mod", name: "B Mod", state: "same", files: [] },
      {
        unique_id: "c.mod",
        name: "C Mod",
        state: "only_second",
        files: ["config.json"],
      },
    ]);
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ProfileCompareCard />
      </QueryClientProvider>,
    );
    fireEvent.change(await screen.findByLabelText("First"), {
      target: { value: "a" },
    });
    fireEvent.change(screen.getByLabelText("Second"), {
      target: { value: "b" },
    });
    expect(
      await screen.findByText(/different config\.json/),
    ).toBeInTheDocument();
    expect(screen.getByText(/settings only in Co-op/)).toBeInTheDocument();
    expect(screen.queryByText("B Mod")).toBeNull();
    expect(compare).toHaveBeenCalledWith("a", "b");
  });
});
