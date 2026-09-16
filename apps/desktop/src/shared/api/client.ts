import {
  BootstrapDto,
  GameInstallationSummaryDto,
  GameInspectionDto,
  ProfileSummaryDto,
  ProfileOverviewDto,
  ModListItemDto,
  ModDetailsDto,
  OperationPreviewDto,
  OperationDto,
  SmapiStatusDto,
  LaunchSessionDto,
  DiagnosticsDto,
} from "./generated";
import { MockBackend } from "@/lib/backend/mock_backend";
import {
  AppSnapshot,
  ArchiveInspectionResult,
  GameInstallation,
  InstalledMod,
  InstallPlan,
  LaunchSession,
  Operation,
  SmapiReleaseInfo,
} from "@/lib/backend/types";

const mock = new MockBackend();

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const api = {
  async bootstrap(): Promise<BootstrapDto> {
    if (!isTauri()) {
      return {
        onboarding_disposition: "completed",
        active_game_installation_id: "mock-steam-game",
        active_profile_id: "00000000-0000-0000-0000-000000000001",
        recovery_summary: null,
        app_version: "0.1.0",
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("bootstrap");
  },

  async completeOnboarding(): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("set_onboarding_disposition", { disposition: "completed" });
  },

  async listGameInstallations(): Promise<GameInstallationSummaryDto[]> {
    if (!isTauri()) {
      return [
        {
          id: "mock-steam-game",
          canonical_root:
            "/home/user/.local/share/Steam/steamapps/common/Stardew Valley",
          operating_system: "Linux",
          storefront: "Steam",
          management_mode: "Managed",
          created_at: new Date().toISOString(),
        },
      ];
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_game_installations");
  },

  async discoverGameInstallations(): Promise<GameInspectionDto[]> {
    if (!isTauri()) {
      return [
        {
          candidate_path:
            "/home/user/.local/share/Steam/steamapps/common/Stardew Valley",
          storefront: "steam",
          detected_version: "1.6.14",
          support_state: "supported_fresh",
          is_usable: true,
          has_existing_smapi: false,
          has_existing_mods: false,
          is_writable: true,
          evidence: ["Game executable found", "Game directory is writable"],
        },
      ];
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("discover_game_installations");
  },

  async registerGameInstallation(
    path: string,
    storefront = "Manual",
  ): Promise<GameInstallationSummaryDto> {
    if (!isTauri()) {
      return {
        id: `game-${Date.now()}`,
        canonical_root: path,
        operating_system: "Linux",
        storefront,
        management_mode: "Managed",
        created_at: new Date().toISOString(),
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("register_game_installation", { path, storefront });
  },

  async validateGameInstallationPath(path: string): Promise<GameInspectionDto> {
    if (!isTauri()) {
      return {
        candidate_path: path,
        storefront: "Manual",
        detected_version: "1.6.14",
        support_state: "SupportedFresh",
        is_usable: true,
        has_existing_smapi: false,
        has_existing_mods: false,
        is_writable: true,
        evidence: ["Game executable found"],
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("validate_game_installation_path", { path });
  },

  async listProfiles(): Promise<ProfileSummaryDto[]> {
    if (!isTauri()) {
      return [
        {
          id: "00000000-0000-0000-0000-000000000001",
          game_installation_id: "mock-steam-game",
          name: "Default Profile",
          description: null,
          revision: 1,
          mod_count: 0,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
          state: "Active",
        },
      ];
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_profiles");
  },

  async createProfile(
    name: string,
    gameInstallationId: string,
  ): Promise<ProfileSummaryDto> {
    if (!isTauri()) {
      return {
        id: `prof-${Date.now()}`,
        game_installation_id: gameInstallationId,
        name,
        description: null,
        revision: 1,
        mod_count: 0,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        state: "Active",
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("create_profile", { name, gameId: gameInstallationId });
  },

  async activateProfile(profileId: string): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("activate_profile", { profileId });
  },

  async archiveProfile(profileId: string): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("archive_profile", { profileId });
  },

  async listArchivedProfiles(): Promise<ProfileSummaryDto[]> {
    if (!isTauri()) return [];
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_archived_profiles");
  },

  async restoreProfile(profileId: string): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("restore_profile", { profileId });
  },

  async getActiveProfileOverview(): Promise<ProfileOverviewDto> {
    if (!isTauri()) {
      return {
        profile: {
          id: "00000000-0000-0000-0000-000000000001",
          game_installation_id: "mock-steam-game",
          name: "Default Profile",
          description: null,
          revision: 1,
          mod_count: 0,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
          state: "Active",
        },
        game: {
          id: "mock-steam-game",
          canonical_root:
            "/home/user/.local/share/Steam/steamapps/common/Stardew Valley",
          operating_system: "Linux",
          storefront: "Steam",
          management_mode: "Managed",
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
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_active_profile_overview");
  },

  async listProfileMods(profileId?: string): Promise<ModListItemDto[]> {
    if (!isTauri()) {
      return [];
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_profile_mods", { profileId });
  },

  async getModDetails(profileComponentId: string): Promise<ModDetailsDto> {
    if (!isTauri()) {
      throw new Error("Mod details not found");
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_mod_details", { profileComponentId });
  },

  async inspectPackageForInstall(
    archivePath: string,
    profileId?: string,
  ): Promise<OperationPreviewDto> {
    if (!isTauri()) {
      return {
        operation_id: `op-${Date.now()}`,
        artifact_hash: "mock-hash",
        original_filename: archivePath.split("/").pop() || "mod.zip",
        byte_size: 1024 * 512,
        detected_components: [
          {
            unique_id: "Mock.Mod",
            name: "Mock Mod",
            author: "MockAuthor",
            version: "1.0.0",
            description: "Mock preview mod",
            relative_root: "MockMod",
          },
        ],
        dependencies_satisfied: true,
        warnings: [],
        blockers: [],
        affected_profile_component_ids: [],
        expected_profile_revision: 1,
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("inspect_package_for_install", { archivePath, profileId });
  },

  async prepareRemoval(
    profileComponentId: string,
  ): Promise<OperationPreviewDto> {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("prepare_remove", { profileComponentId });
  },

  async executeOperation(operationId: string): Promise<OperationDto> {
    if (!isTauri()) {
      return {
        id: operationId,
        kind: "InstallPackage",
        state: "Succeeded",
        game_installation_id: "mock-steam-game",
        profile_id: "00000000-0000-0000-0000-000000000001",
        progress_current: 1,
        progress_total: 1,
        error_code: null,
        error_message: null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        completed_at: new Date().toISOString(),
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("execute_operation", { operationId });
  },

  async listRecentOperations(limit = 50): Promise<OperationDto[]> {
    if (!isTauri()) return [];
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("list_recent_operations", { limit });
  },

  async getOperationDetails(operationId: string): Promise<OperationDto> {
    if (!isTauri()) {
      return {
        id: operationId,
        kind: "InstallPackage",
        state: "Succeeded",
        game_installation_id: "mock-steam-game",
        profile_id: "00000000-0000-0000-0000-000000000001",
        progress_current: 1,
        progress_total: 1,
        error_code: null,
        error_message: null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
        completed_at: new Date().toISOString(),
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_operation_details", { operationId });
  },

  async cancelActiveOperation(operationId: string): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("cancel_active_operation", { operationId });
  },

  async retryRecovery(): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("retry_recovery");
  },

  async getSmapiStatus(gameInstallationId?: string): Promise<SmapiStatusDto> {
    if (!isTauri()) {
      return {
        is_installed: false,
        observed_version: null,
        tested_version: "4.1.10",
        is_compatible: false,
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_smapi_status", { gameId: gameInstallationId });
  },

  async installPinnedSmapi(
    gameInstallationId?: string,
  ): Promise<SmapiStatusDto> {
    if (!isTauri()) {
      return {
        is_installed: true,
        observed_version: "4.1.10",
        tested_version: "4.1.10",
        is_compatible: true,
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("install_pinned_smapi", { gameInstallationId });
  },

  async launchActiveProfile(mode = "Modded"): Promise<LaunchSessionDto> {
    if (!isTauri()) {
      return {
        id: `session-${Date.now()}`,
        state: "mod_load_confirmed",
        profile_id: "00000000-0000-0000-0000-000000000001",
        launched_at: new Date().toISOString(),
        ended_at: null,
        pid: null,
        verified_mods: [],
        verification_details: "All mods loaded",
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("launch_active_profile", { mode });
  },

  async getActiveLaunchSession(): Promise<LaunchSessionDto | null> {
    if (!isTauri()) return null;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_active_launch_session");
  },

  async terminateActiveLaunchSession(sessionId?: string): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("terminate_active_launch_session", { sessionId });
  },

  async getDiagnosticsReport(
    gameInstallationId?: string,
  ): Promise<DiagnosticsDto> {
    if (!isTauri()) {
      return {
        session_id: null,
        session_state: null,
        findings: [],
        raw_log: "[SMAPI] Mock log snippet",
        log_file_path: "~/.config/StardewValley/ErrorLogs/SMAPI-latest.txt",
      };
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_diagnostics_report", { gameInstallationId });
  },

  async pickArchiveDialog(): Promise<string | null> {
    if (!isTauri()) {
      return "/home/user/Downloads/ExampleMod.zip";
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("pick_archive_dialog");
  },

  async pickFolderDialog(): Promise<string | null> {
    if (!isTauri()) {
      return "/home/user/.local/share/Steam/steamapps/common/Stardew Valley";
    }
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("pick_folder_dialog");
  },
};

// Legacy compatibility backend (test-only; production UI uses api above)
export const backend = {
  async cancelInspection(planId: string): Promise<void> {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("cancel_inspection", { planId });
  },
  async getAppSnapshot(gameId?: string): Promise<AppSnapshot> {
    if (!isTauri()) return mock.getAppSnapshot(gameId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_app_snapshot", { gameId });
  },
  async discoverGames(): Promise<GameInstallation[]> {
    if (!isTauri()) return mock.discoverGames();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("discover_games");
  },
  async chooseGame(folderPath: string): Promise<GameInstallation> {
    if (!isTauri()) return mock.chooseGame(folderPath);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("choose_game", { folderPath });
  },
  async selectGame(
    candidatePathOrId: string,
    platformKind?: string,
  ): Promise<AppSnapshot> {
    if (!isTauri()) return mock.selectGame(candidatePathOrId, platformKind);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("select_game", {
      candidatePath: candidatePathOrId,
      candidateId: candidatePathOrId,
      platformKind,
    });
  },
  async prepareSmapi(): Promise<SmapiReleaseInfo> {
    if (!isTauri()) return mock.prepareSmapi();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("prepare_smapi");
  },
  async installSmapi(gameId: string): Promise<SmapiStatusDto> {
    if (!isTauri()) return mock.installSmapi(gameId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("install_smapi", { gameId });
  },
  async pickModFile(): Promise<string | null> {
    if (!isTauri()) return mock.pickModFile();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("pick_mod_file");
  },
  async inspectMod(
    filePath: string,
    setupId: string,
  ): Promise<ArchiveInspectionResult> {
    if (!isTauri()) return mock.inspectMod(filePath, setupId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("inspect_mod", { filePath, setupId });
  },
  async installMod(planOrId: InstallPlan | string): Promise<InstalledMod> {
    if (!isTauri()) return mock.installMod(planOrId);
    const { invoke } = await import("@tauri-apps/api/core");
    const planId = typeof planOrId === "string" ? planOrId : planOrId.plan_id;
    return invoke("install_mod", { planId });
  },
  async removeMod(installedModId: string, setupId: string): Promise<void> {
    if (!isTauri()) return mock.removeMod(installedModId, setupId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("remove_mod", { installedModId, setupId });
  },
  async launchGame(gameId: string, setupId: string): Promise<LaunchSession> {
    if (!isTauri()) return mock.launchGame(gameId, setupId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("launch_game", { gameId, setupId });
  },
  async getOperation(id: string): Promise<Operation | null> {
    if (!isTauri()) return mock.getOperation(id);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_operation", { id });
  },
  async cancelOperation(id: string): Promise<void> {
    if (!isTauri()) return mock.cancelOperation(id);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("cancel_operation", { id });
  },
  async getSession(id: string): Promise<LaunchSession | null> {
    if (!isTauri()) return mock.getSession(id);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_session", { id });
  },
  async pollSession(sessionId: string): Promise<LaunchSession | null> {
    if (!isTauri()) return mock.pollSession(sessionId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("poll_session", { sessionId });
  },
  async terminateGame(sessionId?: string): Promise<void> {
    if (!isTauri()) return mock.terminateGame(sessionId);
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("terminate_game", { sessionId });
  },
  async getSmapiLog(): Promise<string> {
    if (!isTauri()) return mock.getSmapiLog();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_smapi_log");
  },
  async getSmapiLogPath(): Promise<string> {
    if (!isTauri()) return mock.getSmapiLogPath();
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_smapi_log_path");
  },
};
