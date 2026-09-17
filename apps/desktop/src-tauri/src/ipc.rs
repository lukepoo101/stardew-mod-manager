//! The structured Tauri IPC error boundary.
//!
//! Application services return AppError; Tauri commands return IpcResult<T>,
//! which serializes to the generated ApiErrorDto contract. Nothing in between
//! may flatten an application error to a string.
//!
//! Command-local failures that the application layer never sees - malformed
//! request identifiers, a missing active context, an unsupported command
//! argument, a native-dialog channel failure or an unusable local path - are
//! built here with stable codes so the frontend can branch on them instead of
//! parsing prose. These are request-boundary facts, not product policy: the
//! Tauri layer still never invents business rules.

use manager_app::api::dto::ApiErrorDto;
use manager_app::error::{AppError, AppErrorCategory, AppResult};
use std::fmt::Display;

/// The result type of every Tauri product command.
pub type IpcResult<T> = Result<T, ApiErrorDto>;

/// The single conversion from an application result to the IPC contract.
pub trait IntoIpcResult<T> {
    fn into_ipc(self) -> IpcResult<T>;
}

impl<T> IntoIpcResult<T> for AppResult<T> {
    fn into_ipc(self) -> IpcResult<T> {
        self.map_err(ApiErrorDto::from)
    }
}

/// Stable command-boundary error codes.
///
/// These are part of the frontend contract: rename them only together with the
/// code that consumes them.
pub const INVALID_GAME_INSTALLATION_ID: &str = "INVALID_GAME_INSTALLATION_ID";
pub const INVALID_PROFILE_ID: &str = "INVALID_PROFILE_ID";
pub const INVALID_PROFILE_COMPONENT_ID: &str = "INVALID_PROFILE_COMPONENT_ID";
pub const INVALID_OPERATION_ID: &str = "INVALID_OPERATION_ID";
pub const INVALID_LAUNCH_SESSION_ID: &str = "INVALID_LAUNCH_SESSION_ID";
pub const NO_ACTIVE_GAME: &str = "NO_ACTIVE_GAME";
pub const NO_ACTIVE_PROFILE: &str = "NO_ACTIVE_PROFILE";
pub const NO_ACTIVE_LAUNCH_SESSION: &str = "NO_ACTIVE_LAUNCH_SESSION";
pub const PROFILE_NOT_FOUND: &str = "PROFILE_NOT_FOUND";
pub const INVALID_LAUNCH_MODE: &str = "INVALID_LAUNCH_MODE";
pub const MOD_ARCHIVE_PATH_REQUIRED: &str = "MOD_ARCHIVE_PATH_REQUIRED";
pub const MOD_ARCHIVE_NOT_FOUND: &str = "MOD_ARCHIVE_NOT_FOUND";
pub const HOME_DIRECTORY_UNAVAILABLE: &str = "HOME_DIRECTORY_UNAVAILABLE";
pub const NATIVE_DIALOG_FAILED: &str = "NATIVE_DIALOG_FAILED";

/// A malformed request identifier.
///
/// The summary is written for people, the code is for program logic, and the
/// parser output is diagnostic evidence only.
pub fn invalid_identifier(code: &str, summary: &str, details: impl Display) -> AppError {
    AppError::new(code, AppErrorCategory::Validation, summary).with_details(details.to_string())
}

pub fn invalid_game_installation_id(details: impl Display) -> AppError {
    invalid_identifier(
        INVALID_GAME_INSTALLATION_ID,
        "The game installation identifier is invalid",
        details,
    )
}

pub fn invalid_profile_id(details: impl Display) -> AppError {
    invalid_identifier(
        INVALID_PROFILE_ID,
        "The profile identifier is invalid",
        details,
    )
}

pub fn invalid_profile_component_id(details: impl Display) -> AppError {
    invalid_identifier(
        INVALID_PROFILE_COMPONENT_ID,
        "The profile component identifier is invalid",
        details,
    )
}

pub fn invalid_operation_id(details: impl Display) -> AppError {
    invalid_identifier(
        INVALID_OPERATION_ID,
        "The operation identifier is invalid",
        details,
    )
}

pub fn invalid_launch_session_id(details: impl Display) -> AppError {
    invalid_identifier(
        INVALID_LAUNCH_SESSION_ID,
        "The launch session identifier is invalid",
        details,
    )
}

/// A request that needs an active context which has not been established yet.
///
/// This is a request-level precondition, not an internal failure, so it stays
/// in the validation category with terminal recoverability.
pub fn no_active_game() -> AppError {
    AppError::new(
        NO_ACTIVE_GAME,
        AppErrorCategory::Validation,
        "No game installation is active",
    )
}

pub fn no_active_profile() -> AppError {
    AppError::new(
        NO_ACTIVE_PROFILE,
        AppErrorCategory::Validation,
        "No profile is active",
    )
}

pub fn no_active_launch_session() -> AppError {
    AppError::new(
        NO_ACTIVE_LAUNCH_SESSION,
        AppErrorCategory::Validation,
        "No launch session is active",
    )
}

pub fn profile_not_found() -> AppError {
    AppError::new(
        PROFILE_NOT_FOUND,
        AppErrorCategory::Validation,
        "Profile not found",
    )
}

/// An unsupported value for a command argument.
pub fn invalid_launch_mode(value: &str) -> AppError {
    invalid_identifier(
        INVALID_LAUNCH_MODE,
        "The requested launch mode is not supported",
        value,
    )
}

pub fn mod_archive_path_required() -> AppError {
    AppError::new(
        MOD_ARCHIVE_PATH_REQUIRED,
        AppErrorCategory::Validation,
        "No mod archive path was provided",
    )
}

pub fn mod_archive_not_found(summary: &str, details: impl Display) -> AppError {
    AppError::new(MOD_ARCHIVE_NOT_FOUND, AppErrorCategory::Filesystem, summary)
        .with_details(details.to_string())
}

pub fn home_directory_unavailable(details: impl Display) -> AppError {
    AppError::new(
        HOME_DIRECTORY_UNAVAILABLE,
        AppErrorCategory::Runtime,
        "The user home directory is unavailable",
    )
    .with_details(details.to_string())
}

pub fn native_dialog_failed(details: impl Display) -> AppError {
    AppError::new(
        NATIVE_DIALOG_FAILED,
        AppErrorCategory::Internal,
        "The native file dialog did not return a result",
    )
    .with_details(details.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_app::error::Recoverability;

    #[test]
    fn application_errors_cross_the_boundary_without_flattening() {
        let error = AppError::conflict(
            "Profile was modified since the preview was generated",
            "Expected revision 17, but found 18",
        );

        let dto = AppResult::<()>::Err(error).into_ipc().unwrap_err();

        assert_eq!(dto.code, "OPERATION_CONFLICT");
        assert_eq!(dto.category, AppErrorCategory::OperationConflict);
        assert_eq!(dto.recoverability, Recoverability::RetryWithFreshPlan);
        assert_eq!(
            dto.technical_details.as_deref(),
            Some("Expected revision 17, but found 18")
        );
    }

    #[test]
    fn boundary_failures_are_validation_errors_with_stable_codes() {
        let dto = ApiErrorDto::from(invalid_profile_id("invalid character: 'x'"));

        assert_eq!(dto.code, INVALID_PROFILE_ID);
        assert_eq!(dto.category, AppErrorCategory::Validation);
        assert_eq!(dto.recoverability, Recoverability::Terminal);
        assert_eq!(dto.summary, "The profile identifier is invalid");
        assert_eq!(
            dto.technical_details.as_deref(),
            Some("invalid character: 'x'")
        );
    }

    #[test]
    fn missing_active_context_is_not_an_internal_failure() {
        for (dto, expected_code) in [
            (ApiErrorDto::from(no_active_game()), NO_ACTIVE_GAME),
            (ApiErrorDto::from(no_active_profile()), NO_ACTIVE_PROFILE),
            (
                ApiErrorDto::from(no_active_launch_session()),
                NO_ACTIVE_LAUNCH_SESSION,
            ),
        ] {
            assert_eq!(dto.code, expected_code);
            assert_eq!(dto.category, AppErrorCategory::Validation);
            assert_eq!(dto.recoverability, Recoverability::Terminal);
        }
    }

    #[test]
    fn archive_and_dialog_failures_keep_their_own_categories() {
        let missing = ApiErrorDto::from(mod_archive_not_found(
            "The mod archive could not be found",
            "File 'mod.zip' does not exist",
        ));
        assert_eq!(missing.code, MOD_ARCHIVE_NOT_FOUND);
        assert_eq!(missing.category, AppErrorCategory::Filesystem);

        let home = ApiErrorDto::from(home_directory_unavailable("HOME is not set"));
        assert_eq!(home.code, HOME_DIRECTORY_UNAVAILABLE);
        assert_eq!(home.category, AppErrorCategory::Runtime);

        let dialog = ApiErrorDto::from(native_dialog_failed("channel closed"));
        assert_eq!(dialog.code, NATIVE_DIALOG_FAILED);
        assert_eq!(dialog.category, AppErrorCategory::Internal);

        let required = ApiErrorDto::from(mod_archive_path_required());
        assert_eq!(required.code, MOD_ARCHIVE_PATH_REQUIRED);
        assert_eq!(required.category, AppErrorCategory::Validation);
    }
}
