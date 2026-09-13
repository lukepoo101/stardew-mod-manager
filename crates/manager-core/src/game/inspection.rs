use crate::game::model::{OperatingSystem, Storefront};
use crate::ids::GameInstallationId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportState {
    SupportedFresh,
    SupportedManaged,
    ExistingModdedUnmanaged,
    UnsupportedPlatform,
    InvalidGameDirectory,
    Unreadable,
    Unwritable,
    Unknown,
}

impl SupportState {
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::SupportedFresh | Self::SupportedManaged)
    }
}

/// Read-time observation of a candidate or existing game directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameInspection {
    pub installation_id: Option<GameInstallationId>,
    pub canonical_root: PathBuf,
    pub operating_system: OperatingSystem,
    pub storefront: Storefront,
    pub observed_game_version: Option<String>,
    pub observed_smapi_version: Option<String>,
    pub has_existing_smapi: bool,
    pub has_existing_mods: bool,
    pub is_writable: bool,
    pub support_state: SupportState,
    pub evidence: Vec<String>,
    pub inspected_at: DateTime<Utc>,
}

/// Pure helper to determine SupportState based on observed indicators.
pub fn classify_game_support(
    is_valid_game_dir: bool,
    is_supported_os: bool,
    is_writable: bool,
    _has_existing_smapi: bool,
    has_existing_mods: bool,
    is_already_managed: bool,
) -> SupportState {
    if !is_valid_game_dir {
        return SupportState::InvalidGameDirectory;
    }
    if !is_supported_os {
        return SupportState::UnsupportedPlatform;
    }
    if !is_writable {
        return SupportState::Unwritable;
    }
    if is_already_managed {
        return SupportState::SupportedManaged;
    }
    if has_existing_mods {
        return SupportState::ExistingModdedUnmanaged;
    }
    SupportState::SupportedFresh
}
