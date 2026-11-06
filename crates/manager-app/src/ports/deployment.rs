use crate::error::AppResult;
use manager_core::ids::{OperationId, ProfileId};
use manager_core::install::InstallPlan;
use std::path::{Path, PathBuf};

/// The error code of a folder move that did not happen: the folder is still
/// complete where it was. An adapter that cannot finish a move undoes it, and
/// reports this, unless undoing is impossible too (`MOVE_LEFT_SPLIT`).
pub const MOVE_NOT_DONE: &str = "MOVE_NOT_DONE";

/// The error code of a move that could neither be finished nor undone: the
/// folder is complete at the destination and partly at the source.
pub const MOVE_LEFT_SPLIT: &str = "MOVE_LEFT_SPLIT";

/// Where one copy of a deployment folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderLocation {
    /// The profile's Mods folder, which SMAPI loads.
    Mods,
    /// The profile's disabled directory.
    Disabled,
}

/// One copy of a deployment folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderCopy {
    /// For telling the user which folder is meant.
    pub path: PathBuf,
    /// Every file, relative to the folder with `/`, and its size.
    pub files: Vec<(String, u64)>,
}

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

    /// Whether the staged content of an operation is still present.
    ///
    /// Recovery needs this as evidence: a publication that never happened can
    /// only be retried when its staged source is still there.
    fn staged_content_exists(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        relative_path: &str,
    ) -> AppResult<bool>;
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

    /// Whether the folder is in the profile's disabled directory. With
    /// `deployment_exists` it tells a disabled mod apart from a stray folder
    /// of the same name in Mods.
    fn disabled_deployment_exists(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> AppResult<bool>;

    /// One copy of a deployment folder (in Mods or in the disabled
    /// directory): where it is and every file in it with its size, so it can
    /// be compared with what the install recorded. None when it is not there.
    fn read_folder_copy(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
        location: FolderLocation,
    ) -> AppResult<Option<FolderCopy>>;

    /// Moves one copy of a deployment folder into the operation's recovery
    /// folder, keeping it.
    fn quarantine_folder_copy(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
        location: FolderLocation,
    ) -> AppResult<PathBuf>;

    /// Where an operation's copy of a deployment folder is in the recovery
    /// tree, for telling the user.
    fn recovery_deployment_path(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> AppResult<PathBuf>;

    fn recovery_deployment_exists(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> AppResult<bool>;

    fn get_profile_mods_root(&self, profile_id: &ProfileId) -> PathBuf;

    /// An empty Mods folder owned by the manager, for starting SMAPI with no
    /// mods at all. Anything left in it from an earlier test is cleared; no
    /// profile's folder is involved.
    fn prepare_empty_mods_root(&self) -> AppResult<PathBuf> {
        Err(crate::error::AppError::validation(
            "RUNTIME_TEST_UNAVAILABLE",
            "Testing SMAPI on its own is not available here",
        ))
    }
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
