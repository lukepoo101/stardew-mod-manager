import {
  BootstrapDto,
  DeletedProfileDto,
  LocationDto,
  ModSizeDto,
  SettingsComparisonDto,
  SetupPreviewDto,
  StoredCandidateDto,
  RetentionPolicyDto,
  UnfinishedCopyDto,
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
  PreflightDto,
  BundleExportDto,
  BundlePreviewDto,
  BundleImportDto,
  ToggleImpactDto,
  ShareableSettingsDto,
  RestorePointDto,
  RestorePlanDto,
  RestoreResultDto,
  ModProblemDto,
  ConfigBackupDto,
  ReferenceRecipeDto,
  ReplaceResultDto,
  ReinstallResultDto,
  StorageUsageDto,
  ModFilesCheckDto,
  KnownGoodDto,
  SavesDto,
  SaveBackupDto,
  ExperimentDto,
  ModAnnotationDto,
  DismissedFindingDto,
  DismissedSnapshotDto,
  TroubleshootDto,
  CleanupPreviewDto,
  CleanupResultDto,
  DiagnosticsDto,
  BulkToggleResultDto,
  ModRelationsDto,
  ProfileDeletePreviewDto,
  ProfileFreezeDto,
  OperationDetailsDto,
} from "./generated";
import { invokeApi, isTauri } from "./invoke";

/**
 * Browser-only fixtures.
 *
 * The mock values are deliberately neutral: no fixture pretends to be a real
 * user's home directory, and the platform fields are the lowercase contract
 * values the backend sends rather than display strings.
 */
const INACTIVE_TROUBLESHOOT: TroubleshootDto = {
  active: false,
  phase: "inactive",
  step: 0,
  suspects: [],
  enabled_mods: [],
  culprit: null,
  note: null,
  together: [],
  history: [],
};

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
        recovery: null,
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
    unmanaged = false,
  ): Promise<GameInstallationSummaryDto> {
    if (!isTauri()) {
      return {
        id: `game-${Date.now()}`,
        canonical_root: path,
        operating_system: "linux",
        storefront,
        management_mode: unmanaged ? "external_unmanaged" : "managed",
        created_at: new Date().toISOString(),
      };
    }
    return invokeApi<GameInstallationSummaryDto>("register_game_installation", {
      path,
      storefront,
      unmanaged,
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
          is_default: false,
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
        is_default: false,
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

  async updateProfileDetails(
    profileId: string,
    name: string,
    description: string | null,
  ): Promise<ProfileSummaryDto> {
    if (!isTauri()) {
      throw new Error("Renaming needs the desktop app");
    }
    return invokeApi<ProfileSummaryDto>("update_profile_details", {
      profileId,
      name,
      description,
    });
  },

  async getReferenceRecipe(
    profileId: string,
  ): Promise<ReferenceRecipeDto | null> {
    if (!isTauri()) return null;
    return invokeApi<ReferenceRecipeDto | null>("get_reference_recipe", {
      profileId,
    });
  },

  async attachReferenceRecipe(
    profileId: string,
    recipeJson: string,
  ): Promise<ReferenceRecipeDto> {
    if (!isTauri()) throw new Error("References need the desktop app");
    return invokeApi<ReferenceRecipeDto>("attach_reference_recipe", {
      profileId,
      recipeJson,
    });
  },

  async setReferenceDifferenceAccepted(
    profileId: string,
    differenceKey: string,
    accepted: boolean,
  ): Promise<ReferenceRecipeDto> {
    if (!isTauri()) throw new Error("References need the desktop app");
    return invokeApi<ReferenceRecipeDto>("set_reference_difference_accepted", {
      profileId,
      differenceKey,
      accepted,
    });
  },

  async detachReferenceRecipe(profileId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("detach_reference_recipe", { profileId });
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

  /** Marks a profile as the default, or clears the default with `null`. */
  async setDefaultProfile(profileId: string | null): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("set_default_profile", { profileId });
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
          is_default: false,
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
          state: "installed",
          comparison: "same",
          evidence: [],
        },
        health_summary: {
          status: "Healthy",
          warning_count: 0,
          error_count: 0,
          info_count: 0,
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
        files: [],
        replaces: [],
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

  async listDismissedFindings(): Promise<DismissedFindingDto[]> {
    if (!isTauri()) return [];
    return invokeApi<DismissedFindingDto[]>("list_dismissed_findings");
  },

  async dismissFinding(
    fingerprint: string,
    signature: string,
    severity: string,
    previous: DismissedSnapshotDto | null = null,
  ): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("dismiss_finding", {
      fingerprint,
      signature,
      severity,
      previous,
    });
  },

  async restoreFinding(fingerprint: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("restore_finding", { fingerprint });
  },

  async getToggleImpact(
    profileComponentId: string,
    enable: boolean,
  ): Promise<ToggleImpactDto> {
    if (!isTauri()) {
      return { affected_mods: [], dependents: [], unmet_requirements: [] };
    }
    return invokeApi<ToggleImpactDto>("get_toggle_impact", {
      profileComponentId,
      enable,
    });
  },

  async setModEnabled(
    profileComponentId: string,
    enabled: boolean,
  ): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("set_mod_enabled", { profileComponentId, enabled });
  },

  async exportProfileBundle(
    destinationDir: string,
    settingsFor: string[] = [],
    optional: string[] = [],
  ): Promise<BundleExportDto> {
    if (!isTauri()) {
      return {
        path: `${destinationDir}/mock.smm-bundle.zip`,
        component_count: 0,
        package_count: 0,
        missing_packages: [],
        settings_included: [],
      };
    }
    return invokeApi<BundleExportDto>("export_profile_bundle", {
      destinationDir,
      settingsFor,
      optional,
    });
  },

  async listShareableSettings(
    profileId: string,
  ): Promise<ShareableSettingsDto[]> {
    if (!isTauri()) return [];
    return invokeApi<ShareableSettingsDto[]>("list_shareable_settings", {
      profileId,
    });
  },

  async inspectProfileBundle(bundlePath: string): Promise<BundlePreviewDto> {
    if (!isTauri()) {
      return {
        settings_for: [],
        profile_name: "Mock",
        generated_at: "",
        components: [],
        missing_packages: [],
        warnings: [],
      };
    }
    return invokeApi<BundlePreviewDto>("inspect_profile_bundle", {
      bundlePath,
    });
  },

  /** How the settings of the mods two profiles share compare. */
  async compareProfileSettings(
    firstProfileId: string,
    secondProfileId: string,
  ): Promise<SettingsComparisonDto[]> {
    if (!isTauri()) return [];
    return invokeApi<SettingsComparisonDto[]>("compare_profile_settings", {
      firstProfileId,
      secondProfileId,
    });
  },

  /** A new profile holding exactly what a restore point recorded. */
  async recreateProfileFromPoint(
    profileId: string,
    pointId: string,
    name: string,
  ): Promise<BundleImportDto> {
    if (!isTauri()) throw new Error("Recreating needs the desktop app");
    return invokeApi<BundleImportDto>("recreate_profile_from_point", {
      profileId,
      pointId,
      name,
    });
  },

  /** Deleted profiles that can still be brought back. */
  async listDeletedProfiles(): Promise<DeletedProfileDto[]> {
    if (!isTauri()) return [];
    return invokeApi<DeletedProfileDto[]>("list_deleted_profiles");
  },

  /** Recreates a deleted profile from its record in the trash. */
  async bringBackProfile(entry: string): Promise<BundleImportDto> {
    if (!isTauri()) throw new Error("Bringing back needs the desktop app");
    return invokeApi<BundleImportDto>("bring_back_profile", { entry });
  },

  /** Duplicates that were started but not finished. */
  async listUnfinishedCopies(): Promise<UnfinishedCopyDto[]> {
    if (!isTauri()) return [];
    return invokeApi<UnfinishedCopyDto[]>("list_unfinished_copies");
  },

  /** Completes an interrupted duplicate. */
  async finishProfileCopy(profileId: string): Promise<BundleImportDto> {
    if (!isTauri()) throw new Error("Duplicating needs the desktop app");
    return invokeApi<BundleImportDto>("finish_profile_copy", { profileId });
  },

  async cloneProfile(
    profileId: string,
    name: string,
  ): Promise<BundleImportDto> {
    if (!isTauri()) throw new Error("Duplicating needs the desktop app");
    return invokeApi<BundleImportDto>("clone_profile", { profileId, name });
  },

  async listExperiments(): Promise<ExperimentDto[]> {
    if (!isTauri()) return [];
    return invokeApi<ExperimentDto[]>("list_experiments");
  },

  async startExperiment(
    sourceProfileId: string,
    name: string,
  ): Promise<BundleImportDto> {
    if (!isTauri()) throw new Error("Experiments need the desktop app");
    return invokeApi<BundleImportDto>("start_experiment", {
      sourceProfileId,
      name,
    });
  },

  async keepExperiment(profileId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("keep_experiment", { profileId });
  },

  async listSaves(): Promise<SavesDto> {
    if (!isTauri())
      return { saves_dir: null, saves: [], unavailable_links: [] };
    return invokeApi<SavesDto>("list_saves");
  },

  async associateSave(saveId: string, profileId: string | null): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("associate_save", { saveId, profileId });
  },

  async backupSave(saveId: string): Promise<SaveBackupDto> {
    if (!isTauri()) throw new Error("Save backups need the desktop app");
    return invokeApi<SaveBackupDto>("backup_save", { saveId });
  },

  async restoreSaveBackup(backupId: string): Promise<SaveBackupDto> {
    if (!isTauri()) throw new Error("Save backups need the desktop app");
    return invokeApi<SaveBackupDto>("restore_save_backup", { backupId });
  },

  async getKnownGood(profileId: string): Promise<KnownGoodDto | null> {
    if (!isTauri()) return null;
    return invokeApi<KnownGoodDto | null>("get_known_good", { profileId });
  },

  async checkModFiles(profileId: string): Promise<ModFilesCheckDto[]> {
    if (!isTauri()) return [];
    return invokeApi<ModFilesCheckDto[]>("check_mod_files", { profileId });
  },

  /** Accepts a mod folder's changed files as they are now; nothing is touched. */
  async acceptModFiles(
    profileId: string,
    deploymentId: string,
  ): Promise<ModFilesCheckDto> {
    if (!isTauri()) throw new Error("Accepting changes needs the desktop app");
    return invokeApi<ModFilesCheckDto>("accept_mod_files", {
      profileId,
      deploymentId,
    });
  },

  async importProfileBundle(
    bundlePath: string,
    gameId: string,
    profileName: string,
    includeOptional?: string[],
  ): Promise<BundleImportDto> {
    if (!isTauri()) {
      return {
        settings_applied: [],
        declined_optional: [],
        reference_attached: false,
        profile_id: "mock",
        profile_name: profileName,
        installed: [],
        disabled: [],
        failures: [],
      };
    }
    return invokeApi<BundleImportDto>("import_profile_bundle", {
      bundlePath,
      gameId,
      profileName,
      includeOptional,
    });
  },

  async getStorageUsage(): Promise<StorageUsageDto> {
    if (!isTauri()) {
      return {
        profiles: [],
        packages_bytes: 0,
        installer_cache_bytes: 0,
        save_backups_bytes: 0,
        trash_bytes: 0,
      };
    }
    return invokeApi<StorageUsageDto>("get_storage_usage");
  },

  async getCleanupPreview(): Promise<CleanupPreviewDto> {
    if (!isTauri()) {
      return {
        items: [],
        reclaimable_bytes: 0,
        protected_bytes: 0,
        blocked_reason: null,
      };
    }
    return invokeApi<CleanupPreviewDto>("get_cleanup_preview");
  },

  async getRetentionPolicy(): Promise<RetentionPolicyDto> {
    if (!isTauri()) {
      return {
        keep_save_backups: 5,
        keep_settings_backups: 5,
        keep_trash_days: 30,
      };
    }
    return invokeApi<RetentionPolicyDto>("get_retention_policy");
  },

  async setRetentionPolicy(
    policy: RetentionPolicyDto,
  ): Promise<RetentionPolicyDto> {
    if (!isTauri()) return policy;
    return invokeApi<RetentionPolicyDto>("set_retention_policy", { policy });
  },

  async runCleanup(itemIds: string[]): Promise<CleanupResultDto> {
    if (!isTauri()) {
      return { outcomes: [], reclaimed_bytes: 0, complete: true };
    }
    return invokeApi<CleanupResultDto>("run_cleanup", { itemIds });
  },

  async getTroubleshootStatus(): Promise<TroubleshootDto> {
    if (!isTauri()) return INACTIVE_TROUBLESHOOT;
    return invokeApi<TroubleshootDto>("get_troubleshoot_status");
  },

  async startTroubleshoot(): Promise<TroubleshootDto> {
    if (!isTauri()) return INACTIVE_TROUBLESHOOT;
    return invokeApi<TroubleshootDto>("start_troubleshoot");
  },

  async answerTroubleshoot(problemPresent: boolean): Promise<TroubleshootDto> {
    if (!isTauri()) return INACTIVE_TROUBLESHOOT;
    return invokeApi<TroubleshootDto>("answer_troubleshoot", {
      problemPresent,
    });
  },

  async restoreTroubleshoot(): Promise<TroubleshootDto> {
    if (!isTauri()) return INACTIVE_TROUBLESHOOT;
    return invokeApi<TroubleshootDto>("restore_troubleshoot");
  },

  async listModAnnotations(): Promise<ModAnnotationDto[]> {
    if (!isTauri()) return [];
    return invokeApi<ModAnnotationDto[]>("list_mod_annotations");
  },

  /** Where the game and the manager keep things. */
  async getLocations(): Promise<LocationDto[]> {
    if (!isTauri()) return [];
    return invokeApi<LocationDto[]>("get_locations");
  },

  /** Opens one of the listed places in the file manager. */
  async revealLocation(id: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("reveal_location", { id });
  },

  /** The space a mod's folder and stored archive take on disk. */
  async getModSize(profileComponentId: string): Promise<ModSizeDto> {
    if (!isTauri()) return { folder_bytes: null, archive_bytes: null };
    return invokeApi<ModSizeDto>("get_mod_size", { profileComponentId });
  },

  /** Renames a tag on every mod; returns how many mods changed. */
  async renameModTag(from: string, to: string): Promise<number> {
    if (!isTauri()) return 0;
    return invokeApi<number>("rename_mod_tag", { from, to });
  },

  async setModAnnotation(
    annotation: ModAnnotationDto,
  ): Promise<ModAnnotationDto> {
    if (!isTauri()) return annotation;
    return invokeApi<ModAnnotationDto>("set_mod_annotation", {
      uniqueId: annotation.unique_id,
      favourite: annotation.favourite,
      tags: annotation.tags,
      note: annotation.note,
    });
  },

  /** Installs a package the manager already stores. */
  async installStoredPackage(
    profileId: string,
    artifactHash: string,
  ): Promise<void> {
    if (!isTauri()) throw new Error("Installing needs the desktop app");
    return invokeApi<void>("install_stored_package", {
      profileId,
      artifactHash,
    });
  },

  /** Stored packages that provide a mod with this UniqueID. */
  async findStoredMod(
    uniqueId: string,
    minimumVersion: string | null,
  ): Promise<StoredCandidateDto[]> {
    if (!isTauri()) return [];
    return invokeApi<StoredCandidateDto[]>("find_stored_mod", {
      uniqueId,
      minimumVersion,
    });
  },

  /** Which of these package checksums are stored intact. */
  async storedPackages(artifactHashes: string[]): Promise<string[]> {
    if (!isTauri() || artifactHashes.length === 0) return [];
    return invokeApi<string[]>("stored_packages", { artifactHashes });
  },

  async replaceModVersion(
    profileId: string,
    artifactHash: string,
  ): Promise<ReplaceResultDto> {
    if (!isTauri()) throw new Error("Replacing needs the desktop app");
    return invokeApi<ReplaceResultDto>("replace_mod_version", {
      profileId,
      artifactHash,
    });
  },

  async listConfigBackups(
    profileComponentId: string,
  ): Promise<ConfigBackupDto[]> {
    if (!isTauri()) return [];
    return invokeApi<ConfigBackupDto[]>("list_config_backups", {
      profileComponentId,
    });
  },

  async restoreConfigBackup(
    profileComponentId: string,
    backupId: string,
  ): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("restore_config_backup", {
      profileComponentId,
      backupId,
    });
  },

  async listModProblems(profileId: string): Promise<ModProblemDto[]> {
    if (!isTauri()) return [];
    return invokeApi<ModProblemDto[]>("list_mod_problems", { profileId });
  },

  async listRestorePoints(profileId: string): Promise<RestorePointDto[]> {
    if (!isTauri()) return [];
    return invokeApi<RestorePointDto[]>("list_restore_points", { profileId });
  },

  async createRestorePoint(
    profileId: string,
    label: string,
  ): Promise<RestorePointDto> {
    if (!isTauri()) throw new Error("Restore points need the desktop app");
    return invokeApi<RestorePointDto>("create_restore_point", {
      profileId,
      label,
    });
  },

  async deleteRestorePoint(profileId: string, pointId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("delete_restore_point", { profileId, pointId });
  },

  async planRestore(
    profileId: string,
    pointId: string,
  ): Promise<RestorePlanDto> {
    if (!isTauri()) throw new Error("Restore points need the desktop app");
    return invokeApi<RestorePlanDto>("plan_restore", { profileId, pointId });
  },

  async restoreToPoint(
    profileId: string,
    pointId: string,
  ): Promise<RestoreResultDto> {
    if (!isTauri()) throw new Error("Restore points need the desktop app");
    return invokeApi<RestoreResultDto>("restore_to_point", {
      profileId,
      pointId,
    });
  },

  async reinstallMod(profileComponentId: string): Promise<ReinstallResultDto> {
    if (!isTauri()) throw new Error("Reinstalling needs the desktop app");
    return invokeApi<ReinstallResultDto>("reinstall_mod", {
      profileComponentId,
    });
  },

  async revealModFiles(profileComponentId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("reveal_mod_files", { profileComponentId });
  },

  async revealModPackage(profileComponentId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("reveal_mod_package", { profileComponentId });
  },

  async getBulkToggleImpact(
    profileComponentIds: string[],
    enable: boolean,
  ): Promise<ToggleImpactDto> {
    if (!isTauri()) {
      return { affected_mods: [], dependents: [], unmet_requirements: [] };
    }
    return invokeApi<ToggleImpactDto>("get_bulk_toggle_impact", {
      profileComponentIds,
      enable,
    });
  },

  async setModsEnabled(
    profileComponentIds: string[],
    enabled: boolean,
  ): Promise<BulkToggleResultDto> {
    if (!isTauri()) return { changed: [], failed: [] };
    return invokeApi<BulkToggleResultDto>("set_mods_enabled", {
      profileComponentIds,
      enabled,
    });
  },

  async getModRelations(
    profileComponentId: string,
  ): Promise<ModRelationsDto | null> {
    if (!isTauri()) return null;
    return invokeApi<ModRelationsDto | null>("get_mod_relations", {
      profileComponentId,
    });
  },

  async previewProfileDeletion(
    profileId: string,
  ): Promise<ProfileDeletePreviewDto> {
    if (!isTauri()) {
      return {
        profile_id: profileId,
        name: "Mock",
        mod_count: 0,
        folder_bytes: 0,
        packages_kept: 0,
        blocked_reason: null,
      };
    }
    return invokeApi<ProfileDeletePreviewDto>("preview_profile_deletion", {
      profileId,
    });
  },

  async deleteProfile(profileId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("delete_profile", { profileId });
  },

  async getProfileFreeze(profileId: string): Promise<ProfileFreezeDto | null> {
    if (!isTauri()) return null;
    return invokeApi<ProfileFreezeDto | null>("get_profile_freeze", {
      profileId,
    });
  },

  async freezeProfile(
    profileId: string,
    reason: string,
  ): Promise<ProfileFreezeDto> {
    if (!isTauri()) throw new Error("Freezing needs the desktop app");
    return invokeApi<ProfileFreezeDto>("freeze_profile", { profileId, reason });
  },

  async unfreezeProfile(profileId: string): Promise<void> {
    if (!isTauri()) return;
    return invokeApi<void>("unfreeze_profile", { profileId });
  },

  async getOperationHistoryDetails(
    operationId: string,
  ): Promise<OperationDetailsDto | null> {
    if (!isTauri()) return null;
    return invokeApi<OperationDetailsDto | null>(
      "get_operation_history_details",
      { operationId },
    );
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
        rolled_back: false,
        part_of: null,
      };
    }
    return invokeApi<OperationDto>("execute_operation", { operationId });
  },

  async openExternalPage(url: string): Promise<void> {
    if (!isTauri()) {
      window.open(url, "_blank", "noopener");
      return;
    }
    return invokeApi<void>("open_external_page", { url });
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
        rolled_back: false,
        part_of: null,
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
        state: "absent",
        comparison: "absent",
        evidence: [],
      };
    }
    return invokeApi<SmapiStatusDto>("get_smapi_status", {
      gameId: gameInstallationId,
    });
  },

  async installPinnedSmapi(
    gameInstallationId?: string,
    previewedVersion?: string,
  ): Promise<SmapiStatusDto> {
    if (!isTauri()) {
      return {
        is_installed: true,
        observed_version: "4.1.10",
        tested_version: "4.1.10",
        is_compatible: true,
        state: "installed",
        comparison: "same",
        evidence: [],
      };
    }
    return invokeApi<SmapiStatusDto>("install_pinned_smapi", {
      gameInstallationId,
      previewedVersion,
    });
  },

  /** Removes SMAPI from the game folder; mods and profiles are kept. */
  async uninstallSmapi(gameInstallationId: string): Promise<SmapiStatusDto> {
    if (!isTauri()) throw new Error("Removing SMAPI needs the desktop app");
    return invokeApi<SmapiStatusDto>("uninstall_smapi", {
      gameInstallationId,
    });
  },

  /** What SMAPI setup will change, and whether it can run. Changes nothing. */
  async previewSmapiSetup(
    gameInstallationId: string,
  ): Promise<SetupPreviewDto> {
    if (!isTauri()) {
      return {
        game_path: "/games/Stardew Valley",
        smapi_version: "4.1.10",
        smapi_source: "https://github.com/Pathoschild/SMAPI/releases",
        smapi_sha256: "",
        supported_game_version: "1.6",
        installed_smapi: null,
        modifies: ["Runs the official SMAPI installer on the game folder"],
        creates: [],
        reads: [],
        notices: [],
        checks: [],
        can_proceed: true,
      };
    }
    return invokeApi<SetupPreviewDto>("preview_smapi_setup", {
      gameInstallationId,
    });
  },

  /**
   * Starts the active profile. `acknowledgedWarnings` lists the preflight
   * warnings the user reviewed; the backend refuses if they have changed.
   */
  async launchActiveProfile(
    mode = "Modded",
    acknowledgedWarnings?: string[],
  ): Promise<LaunchSessionDto> {
    if (!isTauri()) {
      return {
        id: `session-${Date.now()}`,
        state: "mod_load_confirmed",
        profile_id: "00000000-0000-0000-0000-000000000001",
        launch_mode: mode.toLowerCase() === "vanilla" ? "vanilla" : "modded",
        launched_at: new Date().toISOString(),
        ended_at: null,
        pid: null,
        verified_mods: [],
        verification_details: "All mods loaded",
        acknowledged_warnings: acknowledgedWarnings ?? [],
        expected_mods: [],
        game_version: null,
        smapi_version: null,
      };
    }
    return invokeApi<LaunchSessionDto>("launch_active_profile", {
      mode,
      acknowledgedWarnings,
    });
  },

  async getLaunchPreflight(mode = "Modded"): Promise<PreflightDto> {
    if (!isTauri()) return { can_launch: true, blockers: [], warnings: [] };
    return invokeApi<PreflightDto>("get_launch_preflight", { mode });
  },

  async getLatestLaunchSession(): Promise<LaunchSessionDto | null> {
    if (!isTauri()) return null;
    return invokeApi<LaunchSessionDto | null>("get_latest_launch_session");
  },

  /** The active profile's recent sessions, newest first. */
  async listLaunchSessions(limit = 20): Promise<LaunchSessionDto[]> {
    if (!isTauri()) return [];
    return invokeApi<LaunchSessionDto[]>("list_launch_sessions", { limit });
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
        log_summary: {
          smapi_version: null,
          game_version: null,
          loaded_mod_count: null,
          skipped_mods: [],
          update_notices: [],
          sources: [],
          total_lines: 0,
          errors: [],
        },
        session_id: null,
        session_state: null,
        findings: [],
        raw_log: "[SMAPI] Mock log snippet",
        log_file_path: "~/.config/StardewValley/ErrorLogs/SMAPI-latest.txt",
        host_operating_system: "linux",
        app_data_dir: "/mock/app-data",
        cache_dir: "/mock/cache",
        steam_installations_checked: ["/mock/steam"],
        log_match: "unmatched",
        log_started_at: null,
        log_read_error: null,
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

  /** Several mod archives at once, for a batch install. */
  async pickArchivesDialog(): Promise<string[]> {
    if (!isTauri()) return [];
    return invokeApi<string[]>("pick_archives_dialog");
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
