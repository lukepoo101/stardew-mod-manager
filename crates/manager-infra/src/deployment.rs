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
            return Err(AppError::conflict(
                "Deployment destination already exists",
                dest.display().to_string(),
            ));
        }

        if std::fs::rename(staged_folder, &dest).is_err() {
            copy_dir_all(staged_folder, &dest).map_err(|e| {
                AppError::filesystem("Failed to copy staged folder to deployment", e.to_string())
            })?;
            let _ = std::fs::remove_dir_all(staged_folder);
        }

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

        if source.exists() && std::fs::rename(&source, &target).is_err() {
            copy_dir_all(&source, &target).map_err(|e| {
                AppError::filesystem("Failed to quarantine deployment", e.to_string())
            })?;
            let _ = std::fs::remove_dir_all(&source);
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
            if std::fs::rename(&source, &target).is_err() {
                copy_dir_all(&source, &target).map_err(|e| {
                    AppError::filesystem("Failed to restore quarantined deployment", e.to_string())
                })?;
                let _ = std::fs::remove_dir_all(&source);
            }
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

    fn get_profile_mods_root(&self, profile_id: &ProfileId) -> PathBuf {
        self.paths.profile_mods_dir(profile_id)
    }
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
        return Err(AppError::conflict(
            failure,
            format!("{} already exists", target.display()),
        ));
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::filesystem(failure, e.to_string()))?;
    }

    if std::fs::rename(source, target).is_err() {
        copy_dir_all(source, target).map_err(|e| AppError::filesystem(failure, e.to_string()))?;
        std::fs::remove_dir_all(source)
            .map_err(|e| AppError::filesystem(failure, e.to_string()))?;
    }

    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}
