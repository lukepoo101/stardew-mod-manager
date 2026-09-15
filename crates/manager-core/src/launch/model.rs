use crate::ids::{GameInstallationId, LaunchSessionId, ModUniqueId, ProfileId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchMode {
    Modded,
    Vanilla,
    RuntimeTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
pub struct LaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationResult {
    pub confirmed_mods: Vec<String>,
    pub details: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionVerificationBaseline {
    pub log_path: PathBuf,
    pub initial_mtime: Option<DateTime<Utc>>,
    pub initial_size: u64,
    pub launch_time: DateTime<Utc>,
    #[serde(default)]
    pub initial_inode: Option<u64>,
    #[serde(default)]
    pub expected_mods_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModVerificationEvidence {
    pub unique_id: String,
    pub expected_version: String,
    pub found_in_log: bool,
    pub log_entry: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionVerificationResult {
    pub session_matched: bool,
    pub session_start_time: Option<DateTime<Utc>>,
    pub custom_mods_path_detected: bool,
    pub all_mods_confirmed: bool,
    pub verified_mods: Vec<ModVerificationEvidence>,
    pub error_details: Option<String>,
}

/// A persisted launch session instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSession {
    pub id: LaunchSessionId,
    pub game_installation_id: GameInstallationId,
    pub profile_id: ProfileId,
    pub launch_mode: LaunchMode,
    pub launched_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub pid: Option<u32>,
    pub state: SessionState,
    pub expected_mod_ids: Vec<ModUniqueId>,
    pub log_baseline_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub log_baseline: Option<SessionVerificationBaseline>,
    pub verification_result: Option<VerificationResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightCheck {
    pub can_launch: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}
