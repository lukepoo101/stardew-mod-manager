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

/// The identity of a process the manager started.
///
/// A pid alone is not an identity: Windows and Linux both recycle them, so a
/// later observation of "pid 4242 is alive" says nothing about whether it is
/// still the process that was launched. The kernel creation timestamp is stable
/// for one process incarnation, and the image path narrows it further.
///
/// This is persisted with the launch session rather than kept in memory,
/// because the case that matters most is the manager being restarted while the
/// game it started is still running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Kernel creation time of this exact incarnation, if the platform exposes it.
    ///
    /// The unit is platform-specific and the value is only ever compared with
    /// another reading of the same process, never interpreted.
    #[serde(default)]
    pub creation_time: Option<u64>,
    /// The image the manager started, when the platform reports one.
    #[serde(default)]
    pub image_path: Option<String>,
}

impl ProcessIdentity {
    pub fn new(pid: u32, creation_time: Option<u64>, image_path: Option<String>) -> Self {
        Self {
            pid,
            creation_time,
            image_path,
        }
    }
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
    /// The identity the manager recorded when it started this session.
    ///
    /// The pid is retained for the stored column and for rows written before
    /// identity was persisted; every liveness decision uses this value, because
    /// only it can survive a pid being recycled while the manager was closed.
    #[serde(default)]
    pub process_identity: Option<ProcessIdentity>,
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
