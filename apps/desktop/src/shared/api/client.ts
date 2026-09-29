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
import { invokeApi, isTauri } from "./invoke";

/**
 * Browser-only fixtures.
 *
 * The mock values are deliberately neutral: no fixture pretends to be a real
 * user's home directory, and the platform fields are the lowercase contract
 * values the backend sends rather than display strings.
 */
const MOCK_GAME_ROOT = "/mock/steam/steamapps/common/Stardew Valley";
const MOCK_ARCHIVE = "/mock/downloads/ExampleMod.zip";

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
    return invokeApi<BootstrapDto>("bootstrap");
  },

  async completeOnboarding(): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("set_onboarding_disposition", {
      disposition: "completed",
    });
  },

  async listGameInstallations(): Promise<GameInstallationSummaryDto[]> {
    if (!isTauri()) {
      return [
        {
          id: "mock-steam-game",
          canonical_root: MOCK_GAME_ROOT,
          operating_system: "linux",
          storefront: "steam",
          management_mode: "managed",
          created_at: new Date().toISOString(),
        },
      ];
    }
    return invokeApi<GameInstallationSummaryDto[]>("list_game_installations");
  },

  async discoverGameInstallations(): Promise<GameInspectionDto[]> {
    if (!isTauri()) {
      return [
        {
          candidate_path: MOCK_GAME_ROOT,
          storefront: "steam",
          operating_system: "linux",
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
    return invokeApi<GameInspectionDto[]>("discover_game_installations");
  },

  async registerGameInstallation(
    path: string,
    storefront = "Manual",
  ): Promise<GameInstallationSummaryDto> {
    if (!isTauri()) {
      return {
        id: `game-${Date.now()}`,
        canonical_root: path,
        operating_system: "linux",
        storefront,
        management_mode: "managed",
        created_at: new Date().toISOString(),
      };
    }
    return invokeApi<GameInstallationSummaryDto>("register_game_installation", {
      path,
      storefront,
    });
  },

  async validateGameInstallationPath(path: string): Promise<GameInspectionDto> {
    if (!isTauri()) {
      return {
        candidate_path: path,
        storefront: "manual",
        operating_system: "linux",
        detected_version: "1.6.14",
        support_state: "supported_fresh",
        is_usable: true,
        has_existing_smapi: false,
        has_existing_mods: false,
        is_writable: true,
        evidence: ["Game executable found"],
      };
    }
    return invokeApi<GameInspectionDto>("validate_game_installation_path", {
      path,
    });
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
    return invokeApi<ProfileSummaryDto[]>("list_profiles");
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
    return invokeApi<ProfileSummaryDto>("create_profile", {
      name,
      gameId: gameInstallationId,
    });
  },

  async activateProfile(profileId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("activate_profile", { profileId });
  },

  async archiveProfile(profileId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("archive_profile", { profileId });
  },

  async listArchivedProfiles(): Promise<ProfileSummaryDto[]> {
    if (!isTauri()) return [];
    return invokeApi<ProfileSummaryDto[]>("list_archived_profiles");
  },

  async restoreProfile(profileId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("restore_profile", { profileId });
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
          canonical_root: MOCK_GAME_ROOT,
          operating_system: "linux",
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
      };
    }
    return invokeApi<ProfileOverviewDto>("get_active_profile_overview");
  },

  async listProfileMods(profileId?: string): Promise<ModListItemDto[]> {
    if (!isTauri()) {
      return [];
    }
    return invokeApi<ModListItemDto[]>("list_profile_mods", { profileId });
  },

  async getModDetails(profileComponentId: string): Promise<ModDetailsDto> {
    if (!isTauri()) {
      throw new Error("Mod details not found");
    }
    return invokeApi<ModDetailsDto>("get_mod_details", { profileComponentId });
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
    return invokeApi<OperationPreviewDto>("inspect_package_for_install", {
      archivePath,
      profileId,
    });
  },

  async prepareRemoval(
    profileComponentId: string,
  ): Promise<OperationPreviewDto> {
    return invokeApi<OperationPreviewDto>("prepare_remove", {
      profileComponentId,
    });
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
    return invokeApi<OperationDto>("execute_operation", { operationId });
  },

  async listRecentOperations(limit = 50): Promise<OperationDto[]> {
    if (!isTauri()) return [];
    return invokeApi<OperationDto[]>("list_recent_operations", { limit });
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
    return invokeApi<OperationDto>("get_operation_details", { operationId });
  },

  async cancelActiveOperation(operationId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("cancel_active_operation", { operationId });
  },

  async retryRecovery(): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("retry_recovery");
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
    return invokeApi<SmapiStatusDto>("get_smapi_status", {
      gameId: gameInstallationId,
    });
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
    return invokeApi<SmapiStatusDto>("install_pinned_smapi", {
      gameInstallationId,
    });
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
    return invokeApi<LaunchSessionDto>("launch_active_profile", { mode });
  },

  async getActiveLaunchSession(): Promise<LaunchSessionDto | null> {
    if (!isTauri()) return null;
    return invokeApi<LaunchSessionDto | null>("get_active_launch_session");
  },

  async terminateActiveLaunchSession(sessionId?: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("terminate_active_launch_session", { sessionId });
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
        host_operating_system: "linux",
        app_data_dir: "/mock/app-data",
        cache_dir: "/mock/cache",
        steam_installations_checked: ["/mock/steam"],
        smapi_log_locations: [
          {
            operating_system: "linux",
            context: "SMAPI log (Linux)",
            path: "~/.config/StardewValley/ErrorLogs/SMAPI-latest.txt",
          },
        ],
      };
    }
    return invokeApi<DiagnosticsDto>("get_diagnostics_report", {
      gameInstallationId,
    });
  },

  async pickArchiveDialog(): Promise<string | null> {
    if (!isTauri()) {
      return MOCK_ARCHIVE;
    }
    return invokeApi<string | null>("pick_archive_dialog");
  },

  async pickFolderDialog(): Promise<string | null> {
    if (!isTauri()) {
      return MOCK_GAME_ROOT;
    }
    return invokeApi<string | null>("pick_folder_dialog");
  },
};
