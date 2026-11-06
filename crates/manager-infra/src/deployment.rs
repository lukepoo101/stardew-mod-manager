use crate::paths::AppPaths;
use crate::platform::shared::fs;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::deployment::{
    DeploymentPort, FolderCopy, FolderLocation, StagingPort, MOVE_LEFT_SPLIT, MOVE_NOT_DONE,
};
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
            // A staging tree whose files are still held open by a scanner or a
            // just-closed editor is a transient condition, not a failure.
            let _ = fs::remove_dir_all(&dir);
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

    fn read_folder_copy(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
        location: FolderLocation,
    ) -> AppResult<Option<FolderCopy>> {
        let path = join_within(&self.copy_root(profile_id, location), deployment_rel_path)?;
        if !path.is_dir() {
            return Ok(None);
        }
        let mut files = Vec::new();
        list_files(&path, &path, &mut files)
            .map_err(|e| AppError::filesystem("Could not read a mod folder", e.to_string()))?;
        files.sort();
        Ok(Some(FolderCopy { path, files }))
    }

    fn quarantine_folder_copy(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
        location: FolderLocation,
    ) -> AppResult<PathBuf> {
        let source = join_within(&self.copy_root(profile_id, location), deployment_rel_path)?;
        let target = join_within(
            &self.paths.profile_recovery_dir(profile_id, operation_id),
            deployment_rel_path,
        )?;
        move_deployment(&source, &target, "Failed to move the folder aside")?;
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
            if target.exists() {
                // A move aside that stopped part-way left the live folder
                // partly emptied: fill it in from the moved copy, never
                // overwriting what is still there, then drop the copy.
                restore_from_copy(&source, &target).map_err(|e| {
                    AppError::filesystem(
                        format!(
                            "Failed to put the mod's folder back at {} from {}",
                            target.display(),
                            source.display()
                        ),
                        e.to_string(),
                    )
                })?;
                // The live folder is complete now. A copy that cannot be
                // dropped is only a leftover in the recovery tree, where
                // leftovers belong, so it does not fail the restore.
                let _ = fs::remove_dir_all(&source);
            } else {
                atomic_move_tree(&source, &target, "Failed to restore quarantined deployment")?;
            }
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

    fn disabled_deployment_exists(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> AppResult<bool> {
        Ok(join_within(
            &self.paths.profile_disabled_dir(profile_id),
            deployment_rel_path,
        )?
        .exists())
    }

    fn recovery_deployment_path(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> AppResult<PathBuf> {
        join_within(
            &self.paths.profile_recovery_dir(profile_id, operation_id),
            deployment_rel_path,
        )
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

    fn prepare_empty_mods_root(&self) -> AppResult<PathBuf> {
        let root = self.paths.data_dir().join("runtime-test").join("Mods");
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(|e| {
                AppError::system("RUNTIME_TEST_FOLDER", format!("{}: {e}", root.display()))
            })?;
        }
        std::fs::create_dir_all(&root).map_err(|e| {
            AppError::system("RUNTIME_TEST_FOLDER", format!("{}: {e}", root.display()))
        })?;
        Ok(root)
    }
}

/// A deployment folder already occupies the destination path.
///
/// The profile has to be reconciled (the orphaned folder removed or adopted)
/// before the same plan can be published again, so this is not a retryable
/// condition and not a stale-plan condition either.
impl FilesystemDeploymentAdapter {
    fn copy_root(&self, profile_id: &ProfileId, location: FolderLocation) -> PathBuf {
        match location {
            FolderLocation::Mods => self.paths.profile_mods_dir(profile_id),
            FolderLocation::Disabled => self.paths.profile_disabled_dir(profile_id),
        }
    }
}

/// Every regular file under `dir` with its size, relative to `root` with `/`.
/// Links are skipped, never followed.
fn list_files(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            list_files(root, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push((relative.to_string_lossy().replace('\\', "/"), meta.len()));
        }
    }
    Ok(())
}

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

fn move_not_done(failure: &str, source: &Path, details: String) -> AppError {
    AppError {
        code: MOVE_NOT_DONE.to_string(),
        category: manager_app::error::AppErrorCategory::Filesystem,
        summary: format!(
            "{failure}. Nothing was moved: the folder is still complete at {}",
            source.display()
        ),
        technical_details: Some(details),
        context: None,
        recoverability: manager_app::error::Recoverability::Retryable,
        operation_id: None,
    }
}

#[allow(clippy::result_large_err)]
fn atomic_move_tree(source: &Path, target: &Path, failure: &str) -> AppResult<()> {
    // A rename is attempted first because it is atomic. On Windows it also
    // succeeds across directories on the same volume, which is where every
    // manager-owned staging, deployment and recovery tree lives. Transient
    // sharing violations - an antivirus scanner holding a freshly written file,
    // or the game still having a DLL open - are retried on a bounded schedule
    // before the copy fallback is used.
    let Err(rename_error) = fs::rename_path(source, target) else {
        return Ok(());
    };

    // The fallback copies, promotes the copy, then deletes the original.
    // Whatever stops it part-way, it ends in one of two honest states: the
    // move is undone and the original is complete where it was
    // (`MOVE_NOT_DONE`), or, only if undoing is impossible too, an error
    // naming both places (`MOVE_LEFT_SPLIT`). It never reports a failure
    // while the original has quietly been emptied.
    let parent = target
        .parent()
        .ok_or_else(|| AppError::filesystem(failure, "Target path has no parent directory"))?;
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
        return Err(move_not_done(
            failure,
            source,
            format!("rename failed: {rename_error}; copy failed: {copy_error}"),
        ));
    }
    if let Err(promote_error) = fs::rename_path(&temp, target) {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(move_not_done(
            failure,
            source,
            format!("rename failed: {rename_error}; promoting the copy failed: {promote_error}"),
        ));
    }
    let cleanup_error = match fs::remove_dir_all(source) {
        Ok(()) if !source.exists() && target.exists() => return Ok(()),
        Ok(()) => "the original was still there after it was deleted".to_string(),
        Err(error) => error.to_string(),
    };

    // The copy is in place, but the original could not be (completely)
    // removed, for example a file in it is locked or its folder cannot be
    // changed. A half-deleted original is the worst outcome, so the move is
    // undone: whatever was deleted is copied back, then the copy goes.
    match restore_from_copy(target, source) {
        Ok(()) => {
            let extra = match std::fs::remove_dir_all(target) {
                Ok(()) => String::new(),
                Err(error) => format!(
                    "; a duplicate copy could not be removed from {}: {error}",
                    target.display()
                ),
            };
            Err(move_not_done(
                failure,
                source,
                format!(
                    "rename failed: {rename_error}; removing the original after copying failed: {cleanup_error}; the original was completed again from the copy{extra}"
                ),
            ))
        }
        Err(restore_error) => Err(AppError {
            code: MOVE_LEFT_SPLIT.to_string(),
            category: manager_app::error::AppErrorCategory::Filesystem,
            summary: format!(
                "{failure}, and it could not be undone: the complete folder is at {} and part of it is still at {}",
                target.display(),
                source.display()
            ),
            technical_details: Some(format!(
                "rename failed: {rename_error}; removing the original failed: {cleanup_error}; restoring it failed: {restore_error}"
            )),
            context: None,
            recoverability: manager_app::error::Recoverability::RequiresManualIntervention,
            operation_id: None,
        }),
    }
}

/// Copies back into `original` every file of `copy` that is missing there,
/// never overwriting what is still there (a file that differs was kept or
/// put back by the user), then checks that `original` holds every file of
/// `copy`.
fn restore_from_copy(copy: &Path, original: &Path) -> std::io::Result<()> {
    fn fill(copy: &Path, original: &Path) -> std::io::Result<()> {
        fs::create_dir_all(original)?;
        for entry in std::fs::read_dir(copy)? {
            let entry = entry?;
            let from = entry.path();
            let to = original.join(entry.file_name());
            let meta = std::fs::symlink_metadata(&from)?;
            if meta.is_dir() {
                fill(&from, &to)?;
            } else if std::fs::symlink_metadata(&to).is_err() {
                fs::copy_file(&from, &to)?;
            }
        }
        Ok(())
    }
    fill(copy, original)?;
    let mut expected = Vec::new();
    let mut found = Vec::new();
    list_files(copy, copy, &mut expected)?;
    list_files(original, original, &mut found)?;
    let found: std::collections::HashSet<_> = found.into_iter().map(|(path, _)| path).collect();
    match expected.iter().find(|(path, _)| !found.contains(path)) {
        None => Ok(()),
        Some((path, _)) => Err(std::io::Error::other(format!(
            "{path} could not be put back"
        ))),
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
