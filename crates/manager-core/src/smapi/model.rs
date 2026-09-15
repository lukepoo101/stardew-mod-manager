use crate::ids::GameInstallationId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmapiPlatformPolicy {
    pub url: String,
    pub sha256: String,
    pub installer_path: String,
    pub launcher_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmapiReleasePolicy {
    pub schema_version: u32,
    pub tested_version: String,
    pub tag: String,
    pub commit: String,
    pub supported_game_versions: Vec<String>,
    pub platforms: HashMap<String, SmapiPlatformPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmapiObservation {
    pub is_present: bool,
    pub observed_version: Option<String>,
    pub executable_present: bool,
    pub evidence: Vec<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedSmapiInstallation {
    pub game_installation_id: GameInstallationId,
    pub release_version: String,
    pub release_policy_id: String,
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
