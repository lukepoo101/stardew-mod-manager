use crate::error::AppResult;
use manager_core::ids::{OperationId, ProfileId};
use manager_core::install::InstallPlan;
use std::path::{Path, PathBuf};

pub trait StagingPort: Send + Sync {
    fn create_staging_dir(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
    ) -> AppResult<PathBuf>;

    fn clean_staging_dir(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
    ) -> AppResult<()>;
}

pub trait DeploymentPort: Send + Sync {
    fn publish_deployment(
        &self,
        profile_id: &ProfileId,
        staged_folder: &Path,
        destination_rel_path: &str,
    ) -> AppResult<PathBuf>;

    fn quarantine_deployment(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> AppResult<PathBuf>;

    fn restore_quarantined_deployment(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> AppResult<()>;

    /// Moves a deployment out of the game-visible Mods directory so SMAPI no longer loads it.
    fn disable_deployment(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> AppResult<()>;

    /// Moves a previously disabled deployment back into the game-visible Mods directory.
    fn enable_deployment(&self, profile_id: &ProfileId, deployment_rel_path: &str)
        -> AppResult<()>;

    fn deployment_exists(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> AppResult<bool>;

    fn recovery_deployment_exists(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> AppResult<bool>;

    fn get_profile_mods_root(&self, profile_id: &ProfileId) -> PathBuf;
}

pub trait StagedContentVerifierPort: Send + Sync {
    fn verify_staged(&self, plan: &InstallPlan, staged_dir: &Path) -> AppResult<()>;
}

pub trait ArchiveInspectorPort: Send + Sync {
    fn inspect_and_stage(
        &self,
        zip_path: &Path,
        operation_id: &OperationId,
        staging_dir: &Path,
        installed_manifests: &[(manager_core::ids::ModUniqueId, String)],
        smapi_version: Option<&str>,
    ) -> AppResult<InstallPlan>;
}
