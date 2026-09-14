export type StoreKind = "steam_native" | "manual_folder" | "unsupported";

export interface GameInstallation {
  id: string;
  canonical_root: string;
  platform_kind: StoreKind;
  detected_version?: string;
  validated_at: string;
  is_fresh: boolean;
  is_managed?: boolean;
  validation_error?: string;
}

export interface Setup {
  id: string;
  game_id: string;
  display_name: string;
  relative_mods_dir: string;
  created_at: string;
}

export interface Package {
  hash: string;
  original_filename: string;
  source_kind: string;
  byte_size: number;
  created_at: string;
}

export interface ModDependency {
  unique_id: string;
  minimum_version?: string;
  is_required: boolean;
}

export interface ContentPackFor {
  unique_id: string;
  minimum_version?: string;
}

export interface Manifest {
  unique_id: string;
  name: string;
  author: string;
  version: string;
  description?: string;
  entry_dll?: string;
  minimum_api_version?: string;
  dependencies: ModDependency[];
  content_pack_for?: ContentPackFor;
}

export interface InstalledMod {
  id: string;
  setup_id: string;
  package_id: string;
  unique_id: string;
  name: string;
  author: string;
  version: string;
  description?: string;
  raw_manifest: string;
  relative_target_path: string;
  file_inventory: string[];
  installed_at: string;
}

export interface DependencyFinding {
  unique_id: string;
  required_version?: string;
  installed_version?: string;
  is_required: boolean;
  is_content_pack_framework: boolean;
  satisfied: boolean;
  reason: string;
}

export interface DependencyReport {
  is_installable: boolean;
  smapi_compatible: boolean;
  smapi_required_version?: string;
  current_smapi_version?: string;
  duplicate_id: boolean;
  findings: DependencyFinding[];
}

export interface ComponentManifest {
  manifest: Manifest;
  raw_manifest: string;
  relative_subfolder: string;
}

export interface InstallPlan {
  plan_id: string;
  setup_id: string;
  package_hash: string;
  original_filename: string;
  mod_folder_name: string;
  manifest: Manifest;
  raw_manifest: string;
  file_inventory: string[];
  dependency_report: DependencyReport;
  component_manifests?: ComponentManifest[];
}

export interface ArchiveInspectionResult {
  selection_id: string;
  package_hash: string;
  original_filename: string;
  byte_size: number;
  plan: InstallPlan;
}

export type OperationKind = "smapi_setup" | "mod_install" | "mod_remove" | "game_launch";
export type OperationState = "pending" | "prepared" | "running" | "completed" | "failed" | "recovering";

export interface Operation {
  id: string;
  kind: OperationKind;
  state: OperationState;
  plan_json: string;
  error_json?: string;
  created_at: string;
  updated_at: string;
  schema_version: number;
}

export type SessionState =
  | "starting"
  | "running_unverified"
  | "mod_load_confirmed"
  | "exited"
  | "failed"
  | "verification_unavailable";

export interface VerificationResult {
  confirmed_mods: string[];
  details: string;
  timestamp: string;
}

export interface LaunchSession {
  id: string;
  game_id: string;
  setup_id: string;
  launched_at: string;
  pid?: number;
  state: SessionState;
  expected_mod_ids: string[];
  log_baseline_time?: string;
  verification_result?: VerificationResult;
}

export interface SmapiReleaseInfo {
  version: string;
  asset_url: string;
  sha256: string;
  tag: string;
  commit: string;
  supported_game_version: string;
  installer_exec_path: string;
  launcher_exec_path: string;
}

export interface SmapiStatus {
  is_installed: boolean;
  observed_version: string | null;
  tested_version: string;
  is_compatible: boolean;
}

export interface SmapiInstallationRecord {
  id: string;
  game_id: string;
  release_version: string;
  adapter_version: string;
  observed_version?: string;
  installed_at: string;
}

export interface AppSnapshot {
  recovery_error?: string;
  selected_game?: GameInstallation;
  setup?: Setup;
  smapi_installed: boolean;
  smapi_version?: string;
  installed_mods: InstalledMod[];
  active_operation?: Operation;
  active_session?: LaunchSession;
}
