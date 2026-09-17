//! Fallible decoding of persisted semantic enum values.
//!
//! Persisted evidence must either be understood or reported as unreadable. A
//! reader that quietly maps an unrecognised literal onto some other valid domain
//! value changes the meaning of the stored evidence - a corrupt operation state
//! would silently become Failed, an unknown operation kind would silently
//! become SmapiSetup - and recovery would then act on a fabricated
//! interpretation of the database.
//!
//! The rule implemented here is therefore:
//!
//! * a known literal decodes to its domain value, including a deliberate
//!   "unknown" literal that maps to a real Unknown variant;
//! * a known historical literal decodes to the value the published migration
//!   assigns it;
//! * anything else is an explicit storage/decode error.
//!
//! Nothing here rewrites the offending row: a reader may not mutate evidence it
//! failed to understand.

use manager_app::error::{AppError, AppErrorCategory};
use manager_core::deployment::{DeploymentState, InstalledReason};
use manager_core::game::{ManagementMode, OperatingSystem, Storefront};
use manager_core::launch::{LaunchMode, SessionState};
use manager_core::operation::{
    AccessMode, OperationKind, OperationState, OperationStepState, ResourceKind,
};
use manager_core::package::AcquisitionSource;
use manager_core::profile::{OnboardingDisposition, ProfileState};

/// Stable storage/decode error codes. These are part of the diagnostics
/// contract, so they are renamed only together with the code that reads them.
pub const PERSISTED_OPERATION_STATE_INVALID: &str = "PERSISTED_OPERATION_STATE_INVALID";
pub const PERSISTED_OPERATION_KIND_INVALID: &str = "PERSISTED_OPERATION_KIND_INVALID";
pub const PERSISTED_STEP_STATE_INVALID: &str = "PERSISTED_STEP_STATE_INVALID";
pub const PERSISTED_RESOURCE_KIND_INVALID: &str = "PERSISTED_RESOURCE_KIND_INVALID";
pub const PERSISTED_ACCESS_MODE_INVALID: &str = "PERSISTED_ACCESS_MODE_INVALID";
pub const PERSISTED_PROFILE_STATE_INVALID: &str = "PERSISTED_PROFILE_STATE_INVALID";
pub const PERSISTED_DEPLOYMENT_STATE_INVALID: &str = "PERSISTED_DEPLOYMENT_STATE_INVALID";
pub const PERSISTED_INSTALLED_REASON_INVALID: &str = "PERSISTED_INSTALLED_REASON_INVALID";
pub const PERSISTED_MANAGEMENT_MODE_INVALID: &str = "PERSISTED_MANAGEMENT_MODE_INVALID";
pub const PERSISTED_OPERATING_SYSTEM_INVALID: &str = "PERSISTED_OPERATING_SYSTEM_INVALID";
pub const PERSISTED_STOREFRONT_INVALID: &str = "PERSISTED_STOREFRONT_INVALID";
pub const PERSISTED_SESSION_STATE_INVALID: &str = "PERSISTED_SESSION_STATE_INVALID";
pub const PERSISTED_LAUNCH_MODE_INVALID: &str = "PERSISTED_LAUNCH_MODE_INVALID";
pub const PERSISTED_ACQUISITION_SOURCE_INVALID: &str = "PERSISTED_ACQUISITION_SOURCE_INVALID";
pub const PERSISTED_ONBOARDING_DISPOSITION_INVALID: &str =
    "PERSISTED_ONBOARDING_DISPOSITION_INVALID";
/// A persisted value that could not be decoded.
///
/// It travels out of a row-mapping closure as the boxed source of a
/// rusqlite::Error::FromSqlConversionFailure, where map_db_err recognises it and
/// turns it into the structured application error it already carries.
#[derive(Debug)]
pub struct PersistedValueError {
    pub code: &'static str,
    pub summary: &'static str,
    pub details: String,
}

impl std::fmt::Display for PersistedValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.details)
    }
}

impl std::error::Error for PersistedValueError {}

/// Builds the storage failure for an unrecognised persisted literal.
///
/// The technical details name the table, the column and the raw value so an
/// operator can inspect the row, while the summary stays generic.
pub fn invalid_persisted_value(
    column_index: usize,
    code: &'static str,
    summary: &'static str,
    table: &'static str,
    column: &'static str,
    raw: &str,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column_index,
        rusqlite::types::Type::Text,
        Box::new(PersistedValueError {
            code,
            summary,
            details: format!("table={table} column={column} value={raw:?}"),
        }),
    )
}

macro_rules! persisted_enum {
    (
        $(#[$meta:meta])*
        $name:ident -> $ty:ty, $code:expr, $summary:expr, {
            $($literal:literal => $variant:expr,)*
        }
    ) => {
        $(#[$meta])*
        pub fn $name(
            value: &str,
            column_index: usize,
            table: &'static str,
            column: &'static str,
        ) -> Result<$ty, rusqlite::Error> {
            match value {
                $($literal => Ok($variant),)*
                other => Err(invalid_persisted_value(
                    column_index,
                    $code,
                    $summary,
                    table,
                    column,
                    other,
                )),
            }
        }
    };
}

const OPERATION_SUMMARY: &str = "Stored operation data could not be read";
const GAME_SUMMARY: &str = "Stored game installation data could not be read";
const PROFILE_SUMMARY: &str = "Stored profile data could not be read";
const DEPLOYMENT_SUMMARY: &str = "Stored deployment data could not be read";
const LAUNCH_SUMMARY: &str = "Stored launch session data could not be read";
persisted_enum! {
    /// Decodes operations.state.
    ///
    /// "completed" and "recovering" are pre-architecture vocabulary that
    /// migration 0007 rewrites; they are accepted with exactly the meaning that
    /// migration assigns them, so a row it left untouched still decodes instead
    /// of failing the whole read.
    parse_operation_state -> OperationState, PERSISTED_OPERATION_STATE_INVALID, OPERATION_SUMMARY, {
        "draft" => OperationState::Draft,
        "prepared" => OperationState::Prepared,
        "running" => OperationState::Running,
        "committing" => OperationState::Committing,
        "succeeded" => OperationState::Succeeded,
        "failed" => OperationState::Failed,
        "cancellation_requested" => OperationState::CancellationRequested,
        "cancelling" => OperationState::Cancelling,
        "cancelled" => OperationState::Cancelled,
        "rolling_back" => OperationState::RollingBack,
        "rolled_back" => OperationState::RolledBack,
        "recovery_required" => OperationState::RecoveryRequired,
        "pending" => OperationState::Draft,
        "recovering" => OperationState::RecoveryRequired,
        "completed" => OperationState::Succeeded,
    }
}

persisted_enum! {
    /// Decodes operations.kind.
    parse_operation_kind -> OperationKind, PERSISTED_OPERATION_KIND_INVALID, OPERATION_SUMMARY, {
        "smapi_setup" => OperationKind::SmapiSetup,
        "mod_install" => OperationKind::ModInstall,
        "mod_remove" => OperationKind::ModRemove,
        "game_launch" => OperationKind::GameLaunch,
        "profile_create" => OperationKind::ProfileCreate,
        "profile_delete" => OperationKind::ProfileDelete,
    }
}

persisted_enum! {
    /// Decodes operation_steps.state.
    parse_operation_step_state -> OperationStepState, PERSISTED_STEP_STATE_INVALID, OPERATION_SUMMARY, {
        "pending" => OperationStepState::Pending,
        "running" => OperationStepState::Running,
        "completed" => OperationStepState::Completed,
        "failed" => OperationStepState::Failed,
    }
}

persisted_enum! {
    /// Decodes operation_resources.resource_kind.
    parse_resource_kind -> ResourceKind, PERSISTED_RESOURCE_KIND_INVALID, OPERATION_SUMMARY, {
        "profile" => ResourceKind::Profile,
        "game_installation" => ResourceKind::GameInstallation,
        "artifact" => ResourceKind::Artifact,
    }
}

persisted_enum! {
    /// Decodes operation_resources.access_mode.
    ///
    /// "exclusive" is a historical spelling of the only exclusive mode this
    /// model has, so it decodes to Write rather than to a fabricated mode.
    parse_access_mode -> AccessMode, PERSISTED_ACCESS_MODE_INVALID, OPERATION_SUMMARY, {
        "read" => AccessMode::Read,
        "write" => AccessMode::Write,
        "exclusive" => AccessMode::Write,
    }
}

persisted_enum! {
    /// Decodes profiles.state.
    parse_profile_state -> ProfileState, PERSISTED_PROFILE_STATE_INVALID, PROFILE_SUMMARY, {
        "active" => ProfileState::Active,
        "archived" => ProfileState::Archived,
        "corrupted" => ProfileState::Corrupted,
    }
}

persisted_enum! {
    /// Decodes profile_deployments.state.
    parse_deployment_state -> DeploymentState, PERSISTED_DEPLOYMENT_STATE_INVALID, DEPLOYMENT_SUMMARY, {
        "present" => DeploymentState::Present,
        "disabled" => DeploymentState::Disabled,
        "missing" => DeploymentState::Missing,
        "externally_modified" => DeploymentState::ExternallyModified,
        "quarantined" => DeploymentState::Quarantined,
    }
}

persisted_enum! {
    /// Decodes profile_components.installed_reason.
    parse_installed_reason -> InstalledReason, PERSISTED_INSTALLED_REASON_INVALID, DEPLOYMENT_SUMMARY, {
        "direct" => InstalledReason::Direct,
        "dependency" => InstalledReason::Dependency,
        "bundle_companion" => InstalledReason::BundleCompanion,
    }
}

persisted_enum! {
    /// Decodes game_installations.management_mode.
    parse_management_mode -> ManagementMode, PERSISTED_MANAGEMENT_MODE_INVALID, GAME_SUMMARY, {
        "managed" => ManagementMode::Managed,
        "external_unmanaged" => ManagementMode::ExternalUnmanaged,
    }
}

persisted_enum! {
    /// Decodes game_installations.operating_system.
    parse_operating_system -> OperatingSystem, PERSISTED_OPERATING_SYSTEM_INVALID, GAME_SUMMARY, {
        "linux" => OperatingSystem::Linux,
        "windows" => OperatingSystem::Windows,
        "macos" => OperatingSystem::MacOS,
    }
}

persisted_enum! {
    /// Decodes game_installations.storefront.
    ///
    /// "unknown" is a deliberate domain value, not a decode failure.
    parse_storefront -> Storefront, PERSISTED_STOREFRONT_INVALID, GAME_SUMMARY, {
        "steam" => Storefront::Steam,
        "gog" => Storefront::Gog,
        "manual" => Storefront::Manual,
        "unknown" => Storefront::Unknown,
    }
}

persisted_enum! {
    /// Decodes launch_sessions.state.
    parse_session_state -> SessionState, PERSISTED_SESSION_STATE_INVALID, LAUNCH_SUMMARY, {
        "starting" => SessionState::Starting,
        "running_unverified" => SessionState::RunningUnverified,
        "mod_load_confirmed" => SessionState::ModLoadConfirmed,
        "exited" => SessionState::Exited,
        "failed" => SessionState::Failed,
        "verification_unavailable" => SessionState::VerificationUnavailable,
    }
}

persisted_enum! {
    /// Decodes launch_sessions.launch_mode.
    parse_launch_mode -> LaunchMode, PERSISTED_LAUNCH_MODE_INVALID, LAUNCH_SUMMARY, {
        "modded" => LaunchMode::Modded,
        "vanilla" => LaunchMode::Vanilla,
        "runtime_test" => LaunchMode::RuntimeTest,
    }
}

persisted_enum! {
    /// Decodes acquisitions.source.
    parse_acquisition_source -> AcquisitionSource, PERSISTED_ACQUISITION_SOURCE_INVALID, DEPLOYMENT_SUMMARY, {
        "local_file" => AcquisitionSource::LocalFile,
        "direct_url" => AcquisitionSource::DirectUrl,
        "provider" => AcquisitionSource::Provider,
        "manual_reference" => AcquisitionSource::ManualReference,
    }
}

persisted_enum! {
    /// Decodes app_context.onboarding_disposition.
    parse_onboarding_disposition -> OnboardingDisposition, PERSISTED_ONBOARDING_DISPOSITION_INVALID, PROFILE_SUMMARY, {
        "not_started" => OnboardingDisposition::NotStarted,
        "completed" => OnboardingDisposition::Completed,
        "skipped" => OnboardingDisposition::Skipped,
    }
}
/// Converts a storage failure into the application's structured error contract.
///
/// Decode failures carry their own stable code, category and diagnostics; every
/// other SQLite or lock failure keeps the generic storage error.
pub fn map_db_err<E: StorageFailure>(error: E) -> AppError {
    error.into_app_error()
}

/// The storage-layer failures the SQLite adapter can produce.
pub trait StorageFailure {
    fn into_app_error(self) -> AppError;
}

impl StorageFailure for rusqlite::Error {
    fn into_app_error(self) -> AppError {
        if let rusqlite::Error::FromSqlConversionFailure(_, _, source) = &self {
            if let Some(persisted) = source.downcast_ref::<PersistedValueError>() {
                return AppError::new(persisted.code, AppErrorCategory::Storage, persisted.summary)
                    .with_details(persisted.details.clone());
            }
        }
        AppError::new("DB_ERROR", AppErrorCategory::Storage, self.to_string())
    }
}

impl<T> StorageFailure for std::sync::PoisonError<T> {
    fn into_app_error(self) -> AppError {
        AppError::new("DB_ERROR", AppErrorCategory::Storage, self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_literals_decode_to_their_domain_value() {
        assert_eq!(
            parse_operation_state("recovery_required", 0, "operations", "state").unwrap(),
            OperationState::RecoveryRequired
        );
        assert_eq!(
            parse_operation_kind("mod_remove", 0, "operations", "kind").unwrap(),
            OperationKind::ModRemove
        );
        assert_eq!(
            parse_storefront("unknown", 0, "game_installations", "storefront").unwrap(),
            Storefront::Unknown
        );
        assert_eq!(
            parse_access_mode("write", 0, "operation_resources", "access_mode").unwrap(),
            AccessMode::Write
        );
    }

    #[test]
    fn historical_literals_decode_with_the_meaning_the_migration_assigns_them() {
        assert_eq!(
            parse_operation_state("completed", 0, "operations", "state").unwrap(),
            OperationState::Succeeded
        );
        assert_eq!(
            parse_operation_state("recovering", 0, "operations", "state").unwrap(),
            OperationState::RecoveryRequired
        );
        assert_eq!(
            parse_operation_state("pending", 0, "operations", "state").unwrap(),
            OperationState::Draft
        );
    }

    #[test]
    fn an_unrecognised_literal_is_a_typed_storage_error() {
        let error = parse_operation_state("definitely_not_a_state", 2, "operations", "state")
            .expect_err("an unrecognised state must not decode");

        let app = map_db_err(error);
        assert_eq!(app.code, PERSISTED_OPERATION_STATE_INVALID);
        assert_eq!(app.category, AppErrorCategory::Storage);
        assert_eq!(app.summary, OPERATION_SUMMARY);
        let details = app.technical_details.expect("diagnostics");
        assert!(details.contains("table=operations"));
        assert!(details.contains("column=state"));
        assert!(details.contains("definitely_not_a_state"));
    }

    #[test]
    fn an_unrecognised_kind_never_becomes_smapi_setup() {
        let app = map_db_err(
            parse_operation_kind("mystery_kind", 1, "operations", "kind")
                .expect_err("an unrecognised kind must not decode"),
        );

        assert_eq!(app.code, PERSISTED_OPERATION_KIND_INVALID);
        assert_eq!(app.category, AppErrorCategory::Storage);
    }

    #[test]
    fn ordinary_storage_failures_keep_the_generic_code() {
        let app = map_db_err(rusqlite::Error::QueryReturnedNoRows);
        assert_eq!(app.code, "DB_ERROR");
        assert_eq!(app.category, AppErrorCategory::Storage);
    }

    #[test]
    fn every_decode_code_is_present_in_the_audited_families() {
        for code in [
            PERSISTED_OPERATION_STATE_INVALID,
            PERSISTED_OPERATION_KIND_INVALID,
            PERSISTED_STEP_STATE_INVALID,
            PERSISTED_RESOURCE_KIND_INVALID,
            PERSISTED_ACCESS_MODE_INVALID,
            PERSISTED_PROFILE_STATE_INVALID,
            PERSISTED_DEPLOYMENT_STATE_INVALID,
            PERSISTED_INSTALLED_REASON_INVALID,
            PERSISTED_MANAGEMENT_MODE_INVALID,
            PERSISTED_OPERATING_SYSTEM_INVALID,
            PERSISTED_STOREFRONT_INVALID,
            PERSISTED_SESSION_STATE_INVALID,
            PERSISTED_LAUNCH_MODE_INVALID,
            PERSISTED_ACQUISITION_SOURCE_INVALID,
            PERSISTED_ONBOARDING_DISPOSITION_INVALID,
        ] {
            assert!(code.starts_with("PERSISTED_"), "{code}");
            assert!(code.ends_with("_INVALID"), "{code}");
        }
    }
}
