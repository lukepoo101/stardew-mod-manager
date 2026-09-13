pub mod domain;
pub mod game;
pub mod install;
pub mod launch;
pub mod manifest;
pub mod ports;
pub mod smapi;
pub mod use_cases;

pub use domain::*;
pub use game::{
    create_game_installation, create_managed_game_installation, extract_game_version,
    validate_fresh_game, validate_game_directory, validate_managed_game, GameValidationResult,
};
pub use install::{ArchiveInspectionResult, InstallPlan, RemovalPlan};
pub use launch::{LaunchSpec, SessionVerificationBaseline, SessionVerificationResult};
pub use manifest::{
    evaluate_dependencies, parse_manifest, DependencyFinding, DependencyReport, SmapiVersion,
};
pub use ports::*;
pub use smapi::{
    get_pinned_smapi_release, PINNED_INSTALLER_INTERNAL_PATH, PINNED_SMAPI_COMMIT,
    PINNED_SMAPI_GAME_VERSION, PINNED_SMAPI_SHA256, PINNED_SMAPI_TAG, PINNED_SMAPI_URL,
    PINNED_SMAPI_VERSION, SMAPI_EXECUTABLE_NAME, SMAPI_LAUNCHER_SCRIPT_NAME,
};
pub use use_cases::{uuid_v4, AppSnapshot, CoreUseCases};
