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
    /// The process was gone the first time this run of the manager looked,
    /// so when and how the session ended was not observed.
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub env: Vec<(String, String)>,
}

/// What SMAPI's log showed about the mods of one session.
///
/// Only a log that matched the session has an outcome. "Unavailable" is its
/// own answer: a log that cannot show which mods loaded is neither a success
/// nor a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadOutcome {
    /// Every expected mod was confirmed loaded from the profile's Mods folder.
    Confirmed,
    /// SMAPI loaded mods, but it skipped some or left some out.
    Partial,
    /// SMAPI read a different Mods folder than the one the session asked for.
    WrongModsFolder,
    /// The log matched this session but cannot show which mods loaded, for
    /// example because the Mods path is anonymised or mod IDs are missing.
    Unavailable,
}

impl LoadOutcome {
    pub fn as_key(self) -> &'static str {
        match self {
            LoadOutcome::Confirmed => "confirmed",
            LoadOutcome::Partial => "partial",
            LoadOutcome::WrongModsFolder => "wrong_mods_folder",
            LoadOutcome::Unavailable => "unavailable",
        }
    }
}

/// A mod SMAPI said it skipped, in SMAPI's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSkippedMod {
    /// The name, and version when SMAPI printed one.
    pub name: String,
    pub reason: String,
}

fn log_matched_by_default() -> bool {
    // Rows written before this field existed only carried a result when the
    // log matched, or when the spawn failed, which leaves no pid.
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationResult {
    pub confirmed_mods: Vec<String>,
    pub details: String,
    pub timestamp: DateTime<Utc>,
    /// `None` while SMAPI has not listed the mods it loaded, when the log never
    /// matched the session, and for rows recorded before outcomes existed.
    #[serde(default)]
    pub outcome: Option<LoadOutcome>,
    #[serde(default)]
    pub skipped_mods: Vec<SessionSkippedMod>,
    /// Names of expected mods that were neither confirmed nor skipped.
    #[serde(default)]
    pub unconfirmed_mods: Vec<String>,
    /// Whether SMAPI's log matched this session at all.
    #[serde(default = "log_matched_by_default")]
    pub log_matched: bool,
}

impl VerificationResult {
    /// A result that carries only what the manager itself observed, with no
    /// evidence from SMAPI's log.
    pub fn without_log(details: impl Into<String>) -> Self {
        Self {
            confirmed_mods: Vec::new(),
            details: details.into(),
            timestamp: Utc::now(),
            outcome: None,
            skipped_mods: Vec::new(),
            unconfirmed_mods: Vec::new(),
            log_matched: false,
        }
    }
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionVerificationResult {
    pub session_matched: bool,
    pub session_start_time: Option<DateTime<Utc>>,
    pub custom_mods_path_detected: bool,
    pub all_mods_confirmed: bool,
    pub verified_mods: Vec<ModVerificationEvidence>,
    pub error_details: Option<String>,
    /// What the log showed, once SMAPI has listed the mods it loaded.
    #[serde(default)]
    pub outcome: Option<LoadOutcome>,
    #[serde(default)]
    pub skipped_mods: Vec<SessionSkippedMod>,
    /// Names of expected mods that were neither confirmed nor skipped.
    #[serde(default)]
    pub unconfirmed_mods: Vec<String>,
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
    /// The game and SMAPI versions observed just before the process started.
    /// `None` for sessions recorded before this was kept.
    #[serde(default)]
    pub runtime: Option<super::RuntimeVersions>,
    /// Non-blocking preflight warnings the user reviewed and launched past.
    #[serde(default)]
    pub acknowledged_warnings: Vec<String>,
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
