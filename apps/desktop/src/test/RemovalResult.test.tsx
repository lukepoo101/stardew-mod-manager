import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { ModsView } from "@/features/mods/ModsView";
import { api } from "@/shared/api/client";
import { ApiClientError } from "@/shared/api/errors";
import type {
  ModListItemDto,
  ProfileOverviewDto,
} from "@/shared/api/generated";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

const overview = {
  profile: { id: "p1", name: "Default", revision: 1, mod_count: 1 },
  game: { operating_system: "Linux", storefront: "Steam" },
  mod_count: 1,
  smapi_status: { is_installed: true, is_compatible: true },
} as unknown as ProfileOverviewDto;

const contentPatcher = {
  profile_component_id: "c1",
  deployment_id: "d1",
  unique_id: "Pat.ContentPatcher",
  name: "Content Patcher",
  author: "Pat",
  version: "2.0.0",
  description: null,
  enabled: true,
  installed_reason: "explicit",
} as unknown as ModListItemDto;

function wrap(ui: React.ReactNode) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <MemoryRouter>{ui}</MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("removing a mod", () => {
  it("shows what was removed with an Undo that brings its settings back", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([contentPatcher]);
    vi.spyOn(api, "prepareRemoval").mockResolvedValue({
      operation_id: "op-remove",
      artifact_hash: "h",
      original_filename: "CP.zip",
      byte_size: 1,
      detected_components: [],
      dependencies_satisfied: true,
      warnings: [],
      blockers: [],
      affected_profile_component_ids: ["c1"],
      expected_profile_revision: null,
      files: [],
      replaces: [],
    });
    vi.spyOn(api, "checkModFiles").mockResolvedValue([]);
    vi.spyOn(api, "getDependencyMap").mockResolvedValue([]);
    vi.spyOn(api, "executeOperation").mockResolvedValue({} as never);
    const undo = vi.spyOn(api, "undoRemoval").mockResolvedValue({
      installed: ["Content Patcher"],
      settings_restored: 1,
      settings_problem: null,
    });
    wrap(<ModsView />);
    fireEvent.click(
      await screen.findByRole("button", { name: "Remove Content Patcher" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Remove mod" }));
    expect(
      await screen.findByText(/Removed Content Patcher\. Its folder/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(undo).toHaveBeenCalledWith("op-remove"));
    expect(
      await screen.findByText(/with the settings it had/),
    ).toBeInTheDocument();
  });
});

describe("a folder in the way of a toggle", () => {
  it("explains it and offers to move the other folder aside", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      { ...contentPatcher, enabled: false },
    ]);
    vi.spyOn(api, "getToggleImpact").mockResolvedValue({
      affected_mods: ["Content Patcher"],
      dependents: [],
      unmet_requirements: [],
    });
    vi.spyOn(api, "setModEnabled").mockRejectedValue(
      new ApiClientError({
        code: "MOD_FOLDER_COLLISION",
        category: "validation",
        summary:
          "Content Patcher could not be turned on: the profile's Mods folder already has a different folder named Pat.ContentPatcher",
        technical_details: null,
        context: null,
        recoverability: "terminal",
        operation_id: null,
      }),
    );
    const repair = vi
      .spyOn(api, "moveAsideAndSetModEnabled")
      .mockResolvedValue();
    wrap(<ModsView />);
    fireEvent.click(
      await screen.findByRole("button", { name: /Enable Content Patcher/ }),
    );
    expect(
      await screen.findByText(/already has a different folder/),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Move the other folder aside and enable Content Patcher",
      }),
    );
    await waitFor(() => expect(repair).toHaveBeenCalledWith("c1", true));
    await waitFor(() =>
      expect(screen.queryByText(/already has a different folder/)).toBeNull(),
    );
  });
});
