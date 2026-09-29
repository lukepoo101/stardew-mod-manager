import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ModsView } from "@/features/mods/ModsView";
import { api } from "@/shared/api/client";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => vi.restoreAllMocks());

const overview = {
  profile: { id: "p1", name: "Default", revision: 1, mod_count: 2 },
  game: { operating_system: "Linux", storefront: "Steam" },
  mod_count: 2,
  smapi_status: { is_installed: true, is_compatible: true },
} as unknown as ProfileOverviewDto;

const mod = (id: string, name: string): ModListItemDto =>
  ({
    profile_component_id: id,
    unique_id: `${name}.Id`,
    name,
    author: "a",
    version: "1.0.0",
    description: null,
    enabled: true,
    installed_reason: "explicit",
    deployment_id: "d",
    artifact_hash: "h",
    installed_at: "",
  }) as ModListItemDto;

function renderMods() {
  vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
  vi.spyOn(api, "listProfileMods").mockResolvedValue([
    mod("c1", "Alpha"),
    mod("c2", "Beta"),
  ]);
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

describe("bulk enable and disable", () => {
  it("reviews the combined impact before disabling the selection", async () => {
    const impact = vi.spyOn(api, "getBulkToggleImpact").mockResolvedValue({
      affected_mods: ["Alpha", "Alpha Extra", "Beta"],
      dependents: ["Needy"],
      unmet_requirements: [],
    });
    const apply = vi.spyOn(api, "setModsEnabled").mockResolvedValue({
      changed: ["Alpha", "Alpha Extra", "Beta"],
      failed: [],
    });
    renderMods();
    fireEvent.click(await screen.findByLabelText("Select Alpha"));
    fireEvent.click(screen.getByLabelText("Select Beta"));
    expect(screen.getByText("2 selected")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Disable selected" }));
    await waitFor(() =>
      expect(impact).toHaveBeenCalledWith(["c1", "c2"], false),
    );
    expect(await screen.findByText("Disable 3 mod(s)?")).toBeInTheDocument();
    expect(
      screen.getByText(/will not load while it is disabled: Needy/),
    ).toBeInTheDocument();
    expect(apply).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Disable" }));
    await waitFor(() =>
      expect(apply).toHaveBeenCalledWith(["c1", "c2"], false),
    );
    expect(await screen.findByText("3 mod(s) changed.")).toBeInTheDocument();
    expect(screen.queryByText("2 selected")).toBeNull();
  });

  it("reports mods that could not be changed and keeps the selection", async () => {
    vi.spyOn(api, "getBulkToggleImpact").mockResolvedValue({
      affected_mods: ["Alpha"],
      dependents: [],
      unmet_requirements: [],
    });
    vi.spyOn(api, "setModsEnabled").mockResolvedValue({
      changed: [],
      failed: [{ name: "Alpha", message: "Folder in use" }],
    });
    renderMods();
    fireEvent.click(await screen.findByLabelText("Select Alpha"));
    fireEvent.click(screen.getByRole("button", { name: "Enable selected" }));
    fireEvent.click(await screen.findByRole("button", { name: "Enable" }));
    expect(await screen.findByText("Alpha: Folder in use")).toBeInTheDocument();
    expect(screen.getByText("1 selected")).toBeInTheDocument();
  });
});
