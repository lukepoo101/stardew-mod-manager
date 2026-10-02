//! Profile folders under `setups/`, and the trash they are moved to when a
//! profile is deleted.

use crate::paths::AppPaths;
use crate::platform::shared::fs;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::profile_folders::{ProfileFolderPort, DELETION_RECORD};
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

    fn write_deletion_record(&self, profile_id: &ProfileId, json: &str) -> AppResult<()> {
        let folder = self.paths.profile_dir(profile_id);
        std::fs::create_dir_all(&folder).map_err(|e| {
            AppError::filesystem("Could not create the profile's folder", e.to_string())
        })?;
        std::fs::write(folder.join(DELETION_RECORD), json)
            .map_err(|e| AppError::filesystem("Could not save the profile's record", e.to_string()))
    }

    fn list_deletion_records(&self) -> AppResult<Vec<(String, String)>> {
        let Ok(entries) = std::fs::read_dir(self.trash_dir()) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if let Ok(json) = std::fs::read_to_string(entry.path().join(DELETION_RECORD)) {
                out.push((name, json));
            }
        }
        out.sort();
        Ok(out)
    }

    fn trashed_configs(&self, entry: &str, folder: &str) -> AppResult<Vec<(String, Vec<u8>)>> {
        let plain = |part: &str| {
            Path::new(part)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        };
        if !plain(entry) || !plain(folder) {
            return Err(AppError::validation(
                "TRASH_PATH_REJECTED",
                "That is not a plain folder name",
            ));
        }
        let root = self.trash_dir().join(entry);
        for side in ["Mods", ".disabled"] {
            let candidate = root.join(side).join(folder);
            if candidate.is_dir() {
                return crate::deployed_files::config_files_in(&candidate).map_err(|e| {
                    AppError::filesystem("Could not read the old settings", e.to_string())
                });
            }
        }
        Ok(Vec::new())
    }

    fn forget_deletion_record(&self, entry: &str) -> AppResult<()> {
        if Path::new(entry).components().count() != 1 {
            return Err(AppError::validation(
                "TRASH_PATH_REJECTED",
                "That is not a trash entry",
            ));
        }
        match std::fs::remove_file(self.trash_dir().join(entry).join(DELETION_RECORD)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::filesystem(
                "Could not update the trash",
                e.to_string(),
            )),
        }
    }
}
