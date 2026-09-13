use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind {
    SteamNative,
    ManualFolder,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInstallation {
    pub id: String,
    pub canonical_root: PathBuf,
    pub platform_kind: StoreKind,
    pub detected_version: Option<String>,
    pub validated_at: DateTime<Utc>,
    pub is_fresh: bool,
    #[serde(default)]
    pub is_managed: bool,
    pub validation_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setup {
    pub id: String,
    pub game_id: String,
    pub display_name: String,
    pub relative_mods_dir: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub hash: String,
    pub original_filename: String,
    pub source_kind: String,
    pub byte_size: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModDependency {
    pub unique_id: String,
    pub minimum_version: Option<String>,
    pub is_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPackFor {
    pub unique_id: String,
    pub minimum_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub unique_id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: Option<String>,
    pub entry_dll: Option<String>,
    pub minimum_api_version: Option<String>,
    pub dependencies: Vec<ModDependency>,
    pub content_pack_for: Option<ContentPackFor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledMod {
    pub id: String,
    pub setup_id: String,
    pub package_id: String,
    pub unique_id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: Option<String>,
    pub raw_manifest: String,
    pub relative_target_path: String,
    pub file_inventory: Vec<String>,
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    SmapiSetup,
    ModInstall,
    ModRemove,
    GameLaunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Pending,
    Prepared,
    Running,
    Completed,
    Failed,
    Recovering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    GameNotFresh,
    GameRunning,
    MissingDependency,
    UnsupportedArchive,
    SmapiSetupFailed,
    RecoveryRequired,
    DuplicateMod,
    IoError,
    LockAcquisitionFailed,
    VerificationFailed,
    InvalidPath,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationError {
    pub code: ErrorCode,
    pub message: String,
    pub diagnostic_details: Option<String>,
    pub recoverable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub kind: OperationKind,
    pub state: OperationState,
    pub plan_json: String,
    pub error_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub schema_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Starting,
    RunningUnverified,
    ModLoadConfirmed,
    Exited,
    Failed,
    VerificationUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub confirmed_mods: Vec<String>,
    pub details: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSession {
    pub id: String,
    pub game_id: String,
    pub setup_id: String,
    pub launched_at: DateTime<Utc>,
    pub pid: Option<u32>,
    pub state: SessionState,
    pub expected_mod_ids: Vec<String>,
    pub log_baseline_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub log_baseline: Option<crate::launch::SessionVerificationBaseline>,
    pub verification_result: Option<VerificationResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmapiReleaseInfo {
    pub version: String,
    pub asset_url: String,
    pub sha256: String,
    pub tag: String,
    pub commit: String,
    pub supported_game_version: String,
    pub installer_exec_path: String,
    pub launcher_exec_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmapiInstallationRecord {
    pub id: String,
    pub game_id: String,
    pub release_version: String,
    pub adapter_version: String,
    pub observed_version: Option<String>,
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub schema_version: u32,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub is_maximized: bool,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            schema_version: 1,
            width: 1120,
            height: 760,
            x: 100,
            y: 100,
            is_maximized: false,
        }
    }
}
