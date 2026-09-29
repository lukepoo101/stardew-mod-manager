import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi, afterEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { DiagnosticsView } from "@/features/diagnostics/DiagnosticsView";
import { api } from "@/shared/api/client";

const renderDiagnostics = () =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <DiagnosticsView />
    </QueryClientProvider>,
  );

afterEach(() => vi.restoreAllMocks());

describe("diagnostics report", () => {
  it("renders the SMAPI log, its path and reported findings", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValueOnce({
      profile: {
        id: "profile-1",
        game_installation_id: "game-1",
        name: "Default",
        description: null,
        revision: 1,
        mod_count: 1,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        state: "Active",
      },
      game: {
        id: "game-1",
        canonical_root: "/games/Stardew Valley",
        operating_system: "Linux",
        storefront: "Steam",
        management_mode: "Managed",
        created_at: new Date().toISOString(),
      },
      mod_count: 1,
      smapi_status: {
        is_installed: true,
        observed_version: "4.1.10",
        tested_version: "4.1.10",
        is_compatible: true,
      },
      health_summary: {
        status: "Warning",
        warning_count: 1,
        error_count: 0,
        findings: [],
      },
      last_session: null,
    });
    const report = vi.spyOn(api, "getDiagnosticsReport").mockResolvedValue({
      session_id: null,
      session_state: null,
      findings: [
        {
          id: "finding-1",
          fingerprint: "mod-load-unverified",
          code: "MOD_LOAD_UNVERIFIED",
          severity: "Warning",
          category: "Launch",
          title: "Mod load unverified",
          summary: "Mod load could not be verified from the SMAPI log",
          affected_entities: [],
          evidence: [],
          observed_at: new Date().toISOString(),
        },
      ],
      raw_log: "[12:00:00 TRACE SMAPI] Test Mod 1.0.0 by Author",
      log_file_path: "/logs/SMAPI-latest.txt",
      host_operating_system: "windows",
      app_data_dir: "C:\\Users\\tester\\AppData\\Roaming\\stardew-mod-manager",
      cache_dir: "C:\\Users\\tester\\AppData\\Local\\stardew-mod-manager",
      steam_installations_checked: ["C:\\Program Files (x86)\\Steam"],
      smapi_log_locations: [
        {
          operating_system: "windows",
          context: "SMAPI log (Windows)",
          path: "%APPDATA%\\StardewValley\\ErrorLogs\\SMAPI-latest.txt",
        },
      ],
    });

    renderDiagnostics();

    expect(
      await screen.findByText(/Test Mod 1.0.0 by Author/),
    ).toBeInTheDocument();
    expect(screen.getByText("/logs/SMAPI-latest.txt")).toBeInTheDocument();
    expect(screen.getByText("MOD_LOAD_UNVERIFIED")).toBeInTheDocument();
    expect(
      screen.getByText("Mod load could not be verified from the SMAPI log"),
    ).toBeInTheDocument();
    expect(report).toHaveBeenCalled();
  });

  it("reports the host platform and where the manager looked", async () => {
    vi.spyOn(api, "getActiveProfileOverview").mockResolvedValue({
      profile: {
        id: "profile-1",
        game_installation_id: "game-1",
        name: "Default",
        description: null,
        revision: 1,
        mod_count: 0,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        state: "Active",
      },
      game: {
        id: "game-1",
        canonical_root: "C:\\Games\\Stardew Valley",
        operating_system: "windows",
        storefront: "steam",
        management_mode: "managed",
        created_at: new Date().toISOString(),
      },
      mod_count: 0,
      smapi_status: {
        is_installed: true,
        observed_version: "4.1.10",
        tested_version: "4.1.10",
        is_compatible: true,
      },
      health_summary: {
        status: "Healthy",
        warning_count: 0,
        error_count: 0,
        findings: [],
      },
      last_session: null,
    });
    vi.spyOn(api, "getDiagnosticsReport").mockResolvedValue({
      session_id: null,
      session_state: null,
      findings: [],
      raw_log: "",
      log_file_path:
        "C:\\Users\\tester\\AppData\\Roaming\\StardewValley\\ErrorLogs\\SMAPI-latest.txt",
      host_operating_system: "windows",
      app_data_dir: "C:\\Users\\tester\\AppData\\Roaming\\stardew-mod-manager",
      cache_dir: "C:\\Users\\tester\\AppData\\Local\\stardew-mod-manager",
      steam_installations_checked: ["C:\\Program Files (x86)\\Steam"],
      smapi_log_locations: [
        {
          operating_system: "windows",
          context: "SMAPI log (Windows)",
          path: "%APPDATA%\\StardewValley\\ErrorLogs\\SMAPI-latest.txt",
        },
      ],
    });

    renderDiagnostics();

    // The platform label is the backend's answer, not a UI guess.
    expect(await screen.findByText("Windows")).toBeInTheDocument();
    expect(
      screen.getByText("C:\\Program Files (x86)\\Steam"),
    ).toBeInTheDocument();
    expect(screen.getByText("SMAPI log (Windows)")).toBeInTheDocument();
  });
});
