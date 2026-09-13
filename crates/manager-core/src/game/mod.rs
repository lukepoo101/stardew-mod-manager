pub mod inspection;
pub mod model;

pub use inspection::{classify_game_support, GameInspection, SupportState};
pub use model::{GameInstallation, ManagementMode, OperatingSystem, Storefront};

pub const GAME_APP_ID: &str = "413150";

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GameValidationResult {
    pub is_valid: bool,
    pub is_fresh: bool,
    pub detected_version: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

pub fn validate_managed_game(
    _game_path: &Path,
    _platform_kind: crate::domain::StoreKind,
) -> GameValidationResult {
    GameValidationResult {
        is_valid: true,
        is_fresh: false,
        detected_version: None,
        error_code: None,
        error_message: None,
    }
}

pub fn validate_fresh_game(
    _game_path: &Path,
    _platform_kind: crate::domain::StoreKind,
) -> GameValidationResult {
    GameValidationResult {
        is_valid: true,
        is_fresh: true,
        detected_version: None,
        error_code: None,
        error_message: None,
    }
}

pub fn create_game_installation(
    id: &str,
    canonical_root: PathBuf,
    platform_kind: crate::domain::StoreKind,
) -> crate::domain::GameInstallation {
    crate::domain::GameInstallation {
        id: id.to_string(),
        canonical_root,
        platform_kind,
        detected_version: None,
        validated_at: chrono::Utc::now(),
        is_fresh: true,
        is_managed: false,
        validation_error: None,
    }
}

pub fn create_managed_game_installation(
    id: &str,
    canonical_root: PathBuf,
    platform_kind: crate::domain::StoreKind,
) -> crate::domain::GameInstallation {
    crate::domain::GameInstallation {
        id: id.to_string(),
        canonical_root,
        platform_kind,
        detected_version: None,
        validated_at: chrono::Utc::now(),
        is_fresh: false,
        is_managed: true,
        validation_error: None,
    }
}
