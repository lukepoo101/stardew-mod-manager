import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { ModsView } from "@/features/mods/ModsView";
import { ActivityView } from "@/features/activity/ActivityView";
import { EmptyState } from "@/components/ui/EmptyState";
import { api } from "@/shared/api/client";
import { savePreferences, DEFAULT_PREFERENCES } from "@/shared/preferences";
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

const disabledMod = {
  profile_component_id: "c",
  unique_id: "A.Mod",
  name: "A Mod",
  author: "me",
  version: "1.0.0",
  description: null,
  enabled: false,
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

describe("empty states", () => {
  it("offers the local install path when a profile has no mods", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([]);
    wrap(<ModsView />);
    expect(
      await screen.findByText("No mods in this profile yet"),
    ).toBeInTheDocument();
    expect(screen.getByText(/No account is needed/)).toBeInTheDocument();
    expect(
      screen.getAllByRole("button", { name: "Choose mod ZIP" }).length,
    ).toBeGreaterThan(1);
  });

  it("says a filter hides mods rather than claiming there are none", async () => {
    savePreferences({ ...DEFAULT_PREFERENCES, modFilter: "enabled" });
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([disabledMod]);
    wrap(<ModsView />);
    expect(
      await screen.findByText("No enabled mods match"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Show all mods" }));
    expect(await screen.findByText("A Mod")).toBeInTheDocument();
  });

  it("shows a load failure instead of an empty list", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockRejectedValue(new Error("disk gone"));
    wrap(<ModsView />);
    expect(
      await screen.findByText("Could not load this profile's mods"),
    ).toBeInTheDocument();
    expect(screen.queryByText("No mods in this profile yet")).toBeNull();
  });

  it("points an empty activity log at installing a mod", async () => {
    vi.spyOn(api, "listRecentOperations").mockResolvedValue([]);
    wrap(<ActivityView />);
    expect(
      await screen.findByText("Nothing has happened yet"),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Install a mod" })).toHaveAttribute(
      "href",
      "/app/mods",
    );
  });

  it("shows an undone failure as rolled back, not as an error", async () => {
    vi.spyOn(api, "listRecentOperations").mockResolvedValue([
      {
        id: "op-1",
        kind: "mod_install",
        state: "failed",
        game_installation_id: null,
        profile_id: "p1",
        progress_current: null,
        progress_total: null,
        error_code: "INSTALL_COMPENSATED_STALE_PLAN",
        error_message: null,
        created_at: "2026-09-01T10:00:00Z",
        updated_at: "2026-09-01T10:00:00Z",
        completed_at: "2026-09-01T10:00:00Z",
        rolled_back: true,
        part_of: "Reinstalling Lib",
      },
    ]);
    wrap(<ActivityView />);
    expect(await screen.findByText("rolled back")).toBeInTheDocument();
    expect(screen.getByText("Part of: Reinstalling Lib")).toBeInTheDocument();
    expect(
      screen.getByText(/the changes it had made were\s+undone/),
    ).toBeInTheDocument();
  });

  it("marks a mod whose folder is missing and counts it as needing attention", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      { ...disabledMod, folder_missing: true },
    ] as ModListItemDto[]);
    wrap(<ModsView />);
    expect(await screen.findByText("Folder missing")).toBeInTheDocument();
    expect(screen.getByText(/Needs attention \(\s*1\s*\)/)).toBeInTheDocument();
  });

  it("shows a mod's manifest fields and the raw manifest", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([disabledMod]);
    vi.spyOn(api, "getModDetails").mockResolvedValue({
      profile_component_id: "c",
      unique_id: "A.Mod",
      name: "A Mod",
      author: "me",
      version: "1.0.0",
      description: null,
      entry_dll: null,
      minimum_api_version: "4.0.0",
      minimum_game_version: "1.6.0",
      update_keys: ["Nexus:123"],
      dependencies: [],
      content_pack_for: {
        unique_id: "Pathoschild.ContentPatcher",
        minimum_version: "2.0.0",
      },
      raw_manifest: '{"UniqueID": "A.Mod", "<b>": 1}',
      artifact_hash: "h",
      original_filename: "A.zip",
      deployment_root_path: "/mods/A",
      installed_at: "",
      enabled: false,
      source: null,
      acquired_at: null,
      earlier_versions: [],
    } as never);
    wrap(<ModsView />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: /View manifest details|details/i,
      }),
    );
    expect(await screen.findByText("1.6.0")).toBeInTheDocument();
    expect(screen.getByText("Nexus:123")).toBeInTheDocument();
    expect(
      screen.getByText("Pathoschild.ContentPatcher 2.0.0+"),
    ).toBeInTheDocument();
    // The raw manifest is shown as text, never as markup.
    expect(screen.getByText(/"<b>": 1/)).toBeInTheDocument();
  });

  it("names every mod a package removal takes with it", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue(overview);
    vi.spyOn(api, "listProfileMods").mockResolvedValue([
      {
        ...disabledMod,
        profile_component_id: "c1",
        name: "Core",
        deployment_id: "d1",
        enabled: true,
      },
      {
        ...disabledMod,
        profile_component_id: "c2",
        unique_id: "A.Extra",
        name: "Extra",
        enabled: true,
      },
    ] as ModListItemDto[]);
    vi.spyOn(api, "prepareRemoval").mockResolvedValue({
      operation_id: "op",
      artifact_hash: "h",
      original_filename: "Pack.zip",
      byte_size: 1,
      detected_components: [],
      dependencies_satisfied: true,
      warnings: [],
      blockers: [],
      affected_profile_component_ids: ["c1", "c2"],
      expected_profile_revision: null,
      replaces: [],
    });
    vi.spyOn(api, "checkModFiles").mockResolvedValue([
      {
        deployment_id: "d1",
        mods: ["Core", "Extra"],
        status: "changed",
        missing: [],
        modified: [],
        added: ["notes.txt", "config.json"],
        config_changed: [],
      },
    ]);
    wrap(<ModsView />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove Core" }));
    expect(
      await screen.findByText(/removed together:\s+Core, Extra/),
    ).toBeInTheDocument();
    expect(
      await screen.findByText(/1 file\(s\) the manager\s+did not install/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/moves to the\s+manager's recovery area/),
    ).toBeInTheDocument();
  });

  it("hides guidance text but keeps the action when guidance is off", () => {
    savePreferences({ ...DEFAULT_PREFERENCES, showGuidance: false });
    render(
      <EmptyState
        title="Empty"
        description="Long explanation"
        actions={<button type="button">Do it</button>}
      />,
    );
    expect(screen.queryByText("Long explanation")).toBeNull();
    expect(screen.getByRole("button", { name: "Do it" })).toBeInTheDocument();
  });
});
