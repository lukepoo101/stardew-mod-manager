//! Profile folders under `setups/`, and the trash they are moved to when a
//! profile is deleted.
#![allow(clippy::result_large_err)]

use crate::paths::AppPaths;
use crate::platform::shared::fs;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::profile_folders::{
    ProfileFolderPort, BROUGHT_BACK_RECORD, DELETION_RECORD,
};
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

impl FilesystemProfileFolders {
    /// A trash entry by its plain folder name, never anything outside.
    fn trash_entry(&self, entry: &str) -> AppResult<PathBuf> {
        let plain = matches!(
            Path::new(entry).components().collect::<Vec<_>>().as_slice(),
            [std::path::Component::Normal(_)]
        );
        if !plain {
            return Err(AppError::validation(
                "TRASH_PATH_REJECTED",
                "That is not a trash entry",
            ));
        }
        Ok(self.trash_dir().join(entry))
    }

    /// Trash entries holding a record file of this name. An entry that holds
    /// a brought-back record is never listed as still deleted.
    fn records(&self, file: &str) -> Vec<(String, String)> {
        let Ok(entries) = std::fs::read_dir(self.trash_dir()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if file == DELETION_RECORD && entry.path().join(BROUGHT_BACK_RECORD).exists() {
                continue;
            }
            if let Ok(json) = std::fs::read_to_string(entry.path().join(file)) {
                out.push((name, json));
            }
        }
        out.sort();
        out
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
        Ok(self.records(DELETION_RECORD))
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

    fn mark_brought_back(&self, entry: &str, json: &str) -> AppResult<()> {
        let folder = self.trash_entry(entry)?;
        // The new record first: if the old one cannot be removed after it,
        // the profile is still not offered twice (see `records`).
        std::fs::write(folder.join(BROUGHT_BACK_RECORD), json)
            .map_err(|e| AppError::filesystem("Could not update the trash", e.to_string()))?;
        match std::fs::remove_file(folder.join(DELETION_RECORD)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::filesystem(
                "Could not update the trash",
                e.to_string(),
            )),
        }
    }

    fn list_brought_back_records(&self) -> AppResult<Vec<(String, String)>> {
        Ok(self.records(BROUGHT_BACK_RECORD))
    }

    fn move_recovery_copies(&self, entry: &str, profile_id: &ProfileId) -> AppResult<()> {
        let source = self.trash_entry(entry)?.join(".recovery");
        match std::fs::symlink_metadata(&source) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            // Nothing kept, or a link the manager did not make: leave it.
            _ => return Ok(()),
        }
        let target = self.paths.profile_dir(profile_id).join(".recovery");
        fs::create_dir_all(&target).map_err(|e| {
            AppError::filesystem("Could not prepare the undo copies' folder", e.to_string())
        })?;
        let entries = std::fs::read_dir(&source)
            .map_err(|e| AppError::filesystem("Could not read the undo copies", e.to_string()))?;
        for operation in entries.filter_map(Result::ok) {
            let destination = target.join(operation.file_name());
            if destination.exists() {
                continue;
            }
            // A rename within the data folder: all or nothing per copy.
            fs::rename_path(&operation.path(), &destination).map_err(|e| {
                AppError::filesystem("Could not move an undo copy back", e.to_string())
            })?;
        }
        Ok(())
    }
}
