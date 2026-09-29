//! Profile folders under `setups/`, and the trash they are moved to when a
//! profile is deleted.

use crate::paths::AppPaths;
use crate::platform::shared::fs;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::profile_folders::ProfileFolderPort;
use manager_core::ids::ProfileId;
use std::path::{Path, PathBuf};

pub struct FilesystemProfileFolders {
    paths: AppPaths,
}

impl FilesystemProfileFolders {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn trash_dir(&self) -> PathBuf {
        self.paths.data_dir().join("trash")
    }
}

fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        0
    } else if meta.is_dir() {
        std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|e| size_of(&e.path()))
                    .sum()
            })
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

impl ProfileFolderPort for FilesystemProfileFolders {
    fn folder_size(&self, profile_id: &ProfileId) -> u64 {
        size_of(&self.paths.profile_dir(profile_id))
    }

    fn move_to_trash(&self, profile_id: &ProfileId) -> AppResult<Option<PathBuf>> {
        let folder = self.paths.profile_dir(profile_id);
        match std::fs::symlink_metadata(&folder) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(AppError::filesystem(
                    "Could not read the profile's folder",
                    error.to_string(),
                ))
            }
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AppError::validation(
                    "PROFILE_FOLDER_IS_LINK",
                    "The profile's folder is a link, so it was left alone",
                ))
            }
            Ok(_) => {}
        }
        let trash = self.trash_dir();
        fs::create_dir_all(&trash).map_err(|e| {
            AppError::filesystem("Could not create the trash folder", e.to_string())
        })?;
        let target = trash.join(format!(
            "profile-{}-{}",
            profile_id,
            chrono::Utc::now().format("%Y%m%dT%H%M%S")
        ));
        // A rename within the data folder is atomic, and cannot leave a
        // half-copied profile behind.
        fs::rename_path(&folder, &target).map_err(|e| {
            AppError::filesystem("Could not move the profile's folder aside", e.to_string())
        })?;
        Ok(Some(target))
    }

    fn restore_from_trash(&self, profile_id: &ProfileId, trashed: &Path) -> AppResult<()> {
        if !trashed.starts_with(self.trash_dir()) {
            return Err(AppError::validation(
                "TRASH_PATH_REJECTED",
                "That folder is not in the manager's trash",
            ));
        }
        fs::rename_path(trashed, &self.paths.profile_dir(profile_id)).map_err(|e| {
            AppError::filesystem("Could not put the profile's folder back", e.to_string())
        })
    }
}
