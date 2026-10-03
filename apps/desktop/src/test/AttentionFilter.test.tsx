import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModsView } from "@/features/mods/ModsView";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const mod = (id: string, name: string) =>
  ({
    profile_component_id: id,
    unique_id: `${name}.Id`,
    name,
    author: "a",
    version: "1.0.0",
    description: null,
    enabled: true,
    installed_reason: "direct",
    deployment_id: "d",
    artifact_hash: "h",
    installed_at: "",
  }) as ModListItemDto;

describe("needs attention filter", () => {
  it("shows only mods with an unmet requirement and names it", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Main", revision: 1, mod_count: 2 },
      game: { id: "g1" },
      mod_count: 2,
      smapi_status: { is_installed: true, is_compatible: true },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      mod("c1", "Healthy"),
      mod("c2", "Broken"),
    ]);
    vi.spyOn(api, "listModProblems").mockResolvedValue([
      { profile_component_id: "c2", unmet_requirements: ["Lib.Missing"] },
    ]);
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ModsView />
      </QueryClientProvider>,
    );
    expect(await screen.findByText("Missing requirement")).toBeInTheDocument();
    expect(screen.getByTitle("Needs Lib.Missing")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(/Needs attention \(1\)/));
    await waitFor(() => expect(screen.queryByText("Healthy")).toBeNull());
    expect(screen.getByText("Broken")).toBeInTheDocument();
  });

  it("says when a filter hides a mod that needs attention", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: { id: "p1", name: "Main", revision: 1, mod_count: 2 },
      game: { id: "g1" },
      mod_count: 2,
      smapi_status: { is_installed: true, is_compatible: true },
    } as unknown as ProfileOverviewDto);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      mod("c1", "Healthy"),
      mod("c2", "Broken"),
    ]);
    vi.spyOn(api, "listModProblems").mockResolvedValue([
      { profile_component_id: "c2", unmet_requirements: ["Lib.Missing"] },
    ]);
    render(
      <QueryClientProvider
        client={
          new QueryClient({ defaultOptions: { queries: { retry: false } } })
        }
      >
        <ModsView />
      </QueryClientProvider>,
    );
    await screen.findByText("Broken");
    fireEvent.change(screen.getByPlaceholderText(/search/i), {
      target: { value: "Healthy" },
    });
    expect(
      await screen.findByText(/1 mod\(s\) that need attention are hidden/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Show them" }));
    await waitFor(() => expect(screen.queryByText("Healthy")).toBeNull());
    expect(screen.getByText("Broken")).toBeInTheDocument();
  });
});
