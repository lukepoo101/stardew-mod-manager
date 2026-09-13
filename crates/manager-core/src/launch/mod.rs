use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
    pub env: Vec<(String, String)>,
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
