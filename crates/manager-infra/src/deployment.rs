use crate::paths::AppPaths;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::deployment::{DeploymentPort, StagingPort};
use manager_core::ids::{OperationId, ProfileId};
use std::path::{Component, Path, PathBuf};

/// Joins a caller-supplied relative deployment path onto `root`, refusing anything
/// that would resolve outside of it.
#[allow(clippy::result_large_err)]
fn join_within(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let candidate = Path::new(relative);
    let is_contained = !relative.trim().is_empty()
        && !relative.contains('\0')
        && candidate
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));

    if !is_contained {
        return Err(AppError::validation(
            "INVALID_DEPLOYMENT_PATH",
            format!(
                "Deployment path '{}' escapes the profile directory",
                relative
            ),
        ));
    }

    Ok(root.join(candidate))
}

#[derive(Clone)]
pub struct FilesystemDeploymentAdapter {
    paths: AppPaths,
}

impl FilesystemDeploymentAdapter {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }
}

impl StagingPort for FilesystemDeploymentAdapter {
    fn create_staging_dir(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
    ) -> AppResult<PathBuf> {
        let dir = self.paths.profile_staging_dir(profile_id, operation_id);
        std::fs::create_dir_all(&dir).map_err(|e| {
            AppError::filesystem("Failed to create staging directory", e.to_string())
        })?;
        Ok(dir)
    }

    fn clean_staging_dir(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
    ) -> AppResult<()> {
        let dir = self.paths.profile_staging_dir(profile_id, operation_id);
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        Ok(())
    }

    fn staged_content_exists(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        relative_path: &str,
    ) -> AppResult<bool> {
        let staged = join_within(
            &self.paths.profile_staging_dir(profile_id, operation_id),
            relative_path,
        )?;
        Ok(staged.exists())
    }
}

impl DeploymentPort for FilesystemDeploymentAdapter {
    fn publish_deployment(
        &self,
        profile_id: &ProfileId,
        staged_folder: &Path,
        destination_rel_path: &str,
    ) -> AppResult<PathBuf> {
        let mods_root = self.paths.profile_mods_dir(profile_id);
        std::fs::create_dir_all(&mods_root).map_err(|e| {
            AppError::filesystem("Failed to create profile Mods directory", e.to_string())
        })?;

        let dest = join_within(&mods_root, destination_rel_path)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::filesystem(
                    "Failed to create deployment destination parent",
                    e.to_string(),
                )
            })?;
        }

        if std::fs::symlink_metadata(&dest).is_ok() {
            return Err(deployment_destination_exists(&dest));
        }

        atomic_move_tree(
            staged_folder,
            &dest,
            "Failed to publish staged folder to deployment",
        )?;

        Ok(dest)
    }

    fn quarantine_deployment(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> AppResult<PathBuf> {
        let source = join_within(
            &self.paths.profile_mods_dir(profile_id),
            deployment_rel_path,
        )?;
        let recovery_root = self.paths.profile_recovery_dir(profile_id, operation_id);
        std::fs::create_dir_all(&recovery_root).map_err(|e| {
            AppError::filesystem("Failed to create recovery directory", e.to_string())
        })?;

        let target = join_within(&recovery_root, deployment_rel_path)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::filesystem("Failed to create recovery target parent", e.to_string())
            })?;
        }

        if source.exists() {
            atomic_move_tree(&source, &target, "Failed to quarantine deployment")?;
        } else if !target.exists() {
            return Err(AppError::filesystem(
                "Failed to quarantine deployment",
                format!(
                    "Neither source nor recovery target exists for {}",
                    deployment_rel_path
                ),
            ));
        }

        Ok(target)
    }

    fn restore_quarantined_deployment(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> AppResult<()> {
        let source = join_within(
            &self.paths.profile_recovery_dir(profile_id, operation_id),
            deployment_rel_path,
        )?;
        let target = join_within(
            &self.paths.profile_mods_dir(profile_id),
            deployment_rel_path,
        )?;

        if source.exists() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AppError::filesystem(
                        "Failed to create restore destination parent",
                        e.to_string(),
                    )
                })?;
            }
            atomic_move_tree(&source, &target, "Failed to restore quarantined deployment")?;
        } else if !target.exists() {
            return Err(AppError::filesystem(
                "Failed to restore quarantined deployment",
                format!(
                    "Neither recovery source nor deployment target exists for {}",
                    deployment_rel_path
                ),
            ));
        }

        Ok(())
    }

    fn disable_deployment(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> AppResult<()> {
        let source = join_within(
            &self.paths.profile_mods_dir(profile_id),
            deployment_rel_path,
        )?;
        let target = join_within(
            &self.paths.profile_disabled_dir(profile_id),
            deployment_rel_path,
        )?;
        move_deployment(&source, &target, "Failed to disable deployment")
    }

    fn enable_deployment(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> AppResult<()> {
        let source = join_within(
            &self.paths.profile_disabled_dir(profile_id),
            deployment_rel_path,
        )?;
        let target = join_within(
            &self.paths.profile_mods_dir(profile_id),
            deployment_rel_path,
        )?;
        move_deployment(&source, &target, "Failed to enable deployment")
    }

    fn deployment_exists(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> AppResult<bool> {
        Ok(join_within(
            &self.paths.profile_mods_dir(profile_id),
            deployment_rel_path,
        )?
        .exists())
    }

    fn recovery_deployment_exists(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> AppResult<bool> {
        Ok(join_within(
            &self.paths.profile_recovery_dir(profile_id, operation_id),
            deployment_rel_path,
        )?
        .exists())
    }

    fn get_profile_mods_root(&self, profile_id: &ProfileId) -> PathBuf {
        self.paths.profile_mods_dir(profile_id)
    }
}

/// A deployment folder already occupies the destination path.
///
/// The profile has to be reconciled (the orphaned folder removed or adopted)
/// before the same plan can be published again, so this is not a retryable
/// condition and not a stale-plan condition either.
fn deployment_destination_exists(target: &Path) -> AppError {
    AppError::conflict(
        "DEPLOYMENT_DESTINATION_EXISTS",
        "The profile already has a folder where this mod would be published",
        format!("Deployment destination {} already exists", target.display()),
        manager_app::error::Recoverability::RequiresManualIntervention,
    )
}

#[allow(clippy::result_large_err)]
fn move_deployment(source: &Path, target: &Path, failure: &str) -> AppResult<()> {
    if !source.exists() {
        if target.exists() {
            return Ok(());
        }
        return Err(AppError::filesystem(
            failure,
            format!("Deployment folder {} is missing", source.display()),
        ));
    }

    if target.exists() {
        return Err(deployment_destination_exists(target));
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::filesystem(failure, e.to_string()))?;
    }

    atomic_move_tree(source, target, failure)
}

#[allow(clippy::result_large_err)]
fn atomic_move_tree(source: &Path, target: &Path, failure: &str) -> AppResult<()> {
    match std::fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let parent = target.parent().ok_or_else(|| {
                AppError::filesystem(failure, "Target path has no parent directory")
            })?;
            let temp = parent.join(format!(
                ".{}.publish-{}",
                target
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("deployment"),
                manager_core::uuid_v4()
            ));
            if let Err(copy_error) = copy_tree_no_symlinks(source, &temp) {
                let _ = std::fs::remove_dir_all(&temp);
                return Err(AppError::filesystem(
                    failure,
                    format!(
                        "rename failed: {}; copy failed: {}",
                        rename_error, copy_error
                    ),
                ));
            }
            if let Err(promote_error) = std::fs::rename(&temp, target) {
                let _ = std::fs::remove_dir_all(&temp);
                return Err(AppError::filesystem(
                    failure,
                    format!(
                        "Could not atomically publish copied tree: {}",
                        promote_error
                    ),
                ));
            }
            std::fs::remove_dir_all(source).map_err(|cleanup_error| {
                AppError::filesystem(
                    failure,
                    format!(
                        "Tree was published but source cleanup failed: {}",
                        cleanup_error
                    ),
                )
            })?;
            if source.exists() || !target.exists() {
                return Err(AppError::filesystem(
                    failure,
                    "Move postcondition failed after copy promotion",
                ));
            }
            Ok(())
        }
    }
}

fn copy_tree_no_symlinks(src: &Path, dst: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(src)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "tree source must be a real directory",
        ));
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = dst.join(entry.file_name());
        let ty = std::fs::symlink_metadata(&source_path)?;
        if ty.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "symlink in managed tree",
            ));
        }
        if ty.is_dir() {
            copy_tree_no_symlinks(&source_path, &target_path)?;
        } else if ty.is_file() {
            std::fs::copy(source_path, target_path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsupported entry in managed tree",
            ));
        }
    }
    Ok(())
}
