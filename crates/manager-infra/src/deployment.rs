use crate::paths::AppPaths;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::deployment::{DeploymentPort, StagingPort};
use manager_core::ids::{OperationId, ProfileId};
use std::path::{Path, PathBuf};

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

        let dest = mods_root.join(destination_rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::filesystem(
                    "Failed to create deployment destination parent",
                    e.to_string(),
                )
            })?;
        }

        if dest.exists() {
            let _ = std::fs::remove_dir_all(&dest);
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
        let source = self
            .paths
            .profile_mods_dir(profile_id)
            .join(deployment_rel_path);
        let recovery_root = self.paths.profile_recovery_dir(profile_id, operation_id);
        std::fs::create_dir_all(&recovery_root).map_err(|e| {
            AppError::filesystem("Failed to create recovery directory", e.to_string())
        })?;

        let target = recovery_root.join(deployment_rel_path);
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
        let source = self
            .paths
            .profile_recovery_dir(profile_id, operation_id)
            .join(deployment_rel_path);
        let target = self
            .paths
            .profile_mods_dir(profile_id)
            .join(deployment_rel_path);

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

    fn get_profile_mods_root(&self, profile_id: &ProfileId) -> PathBuf {
        self.paths.profile_mods_dir(profile_id)
    }
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
