//! The persisted execution-step vocabulary of the v2 operation engine.
//!
//! The stored column stays plain text, so this is a typed vocabulary rather than
//! another workflow framework: the engine names the boundaries it crosses, and
//! recovery reads those names back to decide what is already true on disk.
//!
//! Compensation steps are listed here too, but they are only persisted when
//! compensation actually becomes necessary. There are no pre-created rollback
//! steps pretending to have succeeded.

use serde::{Deserialize, Serialize};

/// Step indices of the v2 install lifecycle.
///
/// The preparation steps are persisted before the operation becomes
/// `Prepared`; the execution steps are persisted as they are crossed.
pub const INSTALL_STEP_RETAIN_ARTIFACT: u32 = 1;
pub const INSTALL_STEP_INSPECT_AND_STAGE: u32 = 2;
pub const INSTALL_STEP_VERIFY_STAGED: u32 = 3;
pub const INSTALL_STEP_PUBLISH_DEPLOYMENT: u32 = 4;
pub const INSTALL_STEP_COMMIT_DATABASE: u32 = 5;
pub const INSTALL_STEP_CLEANUP_STAGING: u32 = 6;
pub const INSTALL_STEP_QUARANTINE_PUBLISHED: u32 = 7;

/// Step indices of the v2 removal lifecycle.
pub const REMOVAL_STEP_QUARANTINE_DEPLOYMENT: u32 = 1;
pub const REMOVAL_STEP_COMMIT_DATABASE: u32 = 2;
pub const REMOVAL_STEP_RESTORE_QUARANTINED: u32 = 3;

/// Step indices of the v2 SMAPI lifecycle.
pub const SMAPI_STEP_DOWNLOAD_INSTALLER: u32 = 1;
pub const SMAPI_STEP_INSTALL_FILES: u32 = 2;
pub const SMAPI_STEP_PERSIST_STATE: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStepKind {
    // Install lifecycle.
    RetainArtifact,
    InspectAndStage,
    VerifyStaged,
    PublishDeployment,
    CommitInstallDatabase,
    CleanupStaging,
    QuarantinePublishedDeployment,

    // Removal lifecycle.
    QuarantineDeployment,
    CommitRemovalDatabase,
    RestoreQuarantinedDeployment,

    // SMAPI lifecycle.
    DownloadSmapiInstaller,
    InstallSmapiFiles,
    PersistSmapiState,
}

impl OperationStepKind {
    /// The literal persisted in `operation_steps.step_kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RetainArtifact => "retain_artifact",
            Self::InspectAndStage => "inspect_and_stage",
            Self::VerifyStaged => "verify_staged",
            Self::PublishDeployment => "publish_deployment",
            Self::CommitInstallDatabase => "commit_install_database",
            Self::CleanupStaging => "cleanup_staging",
            Self::QuarantinePublishedDeployment => "quarantine_published_deployment",
            Self::QuarantineDeployment => "quarantine_deployment",
            Self::CommitRemovalDatabase => "commit_removal_database",
            Self::RestoreQuarantinedDeployment => "restore_quarantined_deployment",
            Self::DownloadSmapiInstaller => "download_smapi_installer",
            Self::InstallSmapiFiles => "install_smapi_files",
            Self::PersistSmapiState => "persist_smapi_state",
        }
    }

    /// Reads a persisted step kind.
    ///
    /// The three preparation steps were already persisted by plan-v1 operations
    /// under their original Rust spelling, so those historical literals decode to
    /// the same vocabulary entry instead of being treated as unknown text.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "retain_artifact" | "RetainArtifact" => Some(Self::RetainArtifact),
            "inspect_and_stage" | "InspectAndStage" => Some(Self::InspectAndStage),
            "verify_staged" | "VerifyStaged" => Some(Self::VerifyStaged),
            "publish_deployment" => Some(Self::PublishDeployment),
            "commit_install_database" => Some(Self::CommitInstallDatabase),
            "cleanup_staging" => Some(Self::CleanupStaging),
            "quarantine_published_deployment" => Some(Self::QuarantinePublishedDeployment),
            "quarantine_deployment" => Some(Self::QuarantineDeployment),
            "commit_removal_database" => Some(Self::CommitRemovalDatabase),
            "restore_quarantined_deployment" => Some(Self::RestoreQuarantinedDeployment),
            "download_smapi_installer" => Some(Self::DownloadSmapiInstaller),
            "install_smapi_files" => Some(Self::InstallSmapiFiles),
            "persist_smapi_state" => Some(Self::PersistSmapiState),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_kind_round_trips_through_its_persisted_literal() {
        for kind in [
            OperationStepKind::RetainArtifact,
            OperationStepKind::InspectAndStage,
            OperationStepKind::VerifyStaged,
            OperationStepKind::PublishDeployment,
            OperationStepKind::CommitInstallDatabase,
            OperationStepKind::CleanupStaging,
            OperationStepKind::QuarantinePublishedDeployment,
            OperationStepKind::QuarantineDeployment,
            OperationStepKind::CommitRemovalDatabase,
            OperationStepKind::RestoreQuarantinedDeployment,
            OperationStepKind::DownloadSmapiInstaller,
            OperationStepKind::InstallSmapiFiles,
            OperationStepKind::PersistSmapiState,
        ] {
            assert_eq!(OperationStepKind::parse(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn plan_v1_preparation_literals_still_decode() {
        assert_eq!(
            OperationStepKind::parse("RetainArtifact"),
            Some(OperationStepKind::RetainArtifact)
        );
        assert_eq!(
            OperationStepKind::parse("InspectAndStage"),
            Some(OperationStepKind::InspectAndStage)
        );
        assert_eq!(
            OperationStepKind::parse("VerifyStaged"),
            Some(OperationStepKind::VerifyStaged)
        );
    }

    #[test]
    fn an_unknown_step_kind_has_no_interpretation() {
        assert_eq!(OperationStepKind::parse("DefinitelyNotAStep"), None);
    }
}
