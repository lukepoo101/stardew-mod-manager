//! Lists and removes items inside the manager's own storage roots.
//!
//! Only four kinds of location are ever touched: retained package archives,
//! the SMAPI installer cache, and each profile's staging and recovery folders.
//! A link is never followed or removed, and removal re-checks that the path is
//! a direct child of the root it was found in.

use crate::platform::shared::fs;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::storage::{StorageArea, StorageEntry, StorageInventoryPort};
use std::path::{Path, PathBuf};

pub struct FilesystemStorageInventory {
    data_dir: PathBuf,
    packages_dir: PathBuf,
    smapi_cache_dir: PathBuf,
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
}

/// Bytes on disk below `path`, never following links.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| size_of(&entry.path()))
                .sum()
        })
        .unwrap_or(0)
}

fn children(dir: &Path) -> Vec<(String, PathBuf)> {
    // A root that is itself a link could point anywhere: skip it entirely.
    if is_link(dir) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            Some((name, entry.path()))
        })
        .collect();
    found.sort();
    found
}

/// One plain path segment: no separators, no parent references.
fn segment(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\'])
}

/// A timestamp at the start of a folder name the manager wrote.
fn parse_stamp(name: &str, format: &str, length: usize) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::NaiveDateTime::parse_from_str(name.get(..length)?, format)
        .ok()
        .map(|naive| naive.and_utc())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

impl FilesystemStorageInventory {
    pub fn new(data_dir: PathBuf, packages_dir: PathBuf, smapi_cache_dir: PathBuf) -> Self {
        Self {
            data_dir,
            packages_dir,
            smapi_cache_dir,
        }
    }

    fn setups_dir(&self) -> PathBuf {
        self.data_dir.join("setups")
    }

    fn operation_root(&self, profile: &str, area: StorageArea) -> Option<PathBuf> {
        let folder = match area {
            StorageArea::Staging => ".staging",
            StorageArea::Recovery => ".recovery",
            _ => return None,
        };
        Some(self.setups_dir().join(profile).join(folder))
    }

    /// The only path an entry of this area and key may have.
    fn expected_path(&self, entry: &StorageEntry) -> Option<PathBuf> {
        if entry.key.is_empty()
            || entry.key.contains(['/', '\\'])
            || entry.key == "."
            || entry.key == ".."
        {
            return None;
        }
        match entry.area {
            StorageArea::Package => {
                is_sha256(&entry.key).then(|| self.packages_dir.join(format!("{}.zip", entry.key)))
            }
            StorageArea::SmapiCache => Some(self.smapi_cache_dir.join(&entry.key)),
            StorageArea::Staging | StorageArea::Recovery => {
                let profile = entry.profile_id.as_deref()?;
                if profile.is_empty() || profile.contains(['/', '\\']) || profile.starts_with('.') {
                    return None;
                }
                Some(self.operation_root(profile, entry.area)?.join(&entry.key))
            }
            StorageArea::SaveBackup => {
                let save = entry.group.as_deref().filter(|g| segment(g))?;
                Some(
                    self.data_dir
                        .join("save-backups")
                        .join(save)
                        .join(&entry.key),
                )
            }
            StorageArea::ConfigBackup => {
                let profile = entry.profile_id.as_deref().filter(|p| segment(p))?;
                let mod_id = entry.group.as_deref().filter(|g| segment(g))?;
                Some(
                    self.data_dir
                        .join("config-backups")
                        .join(profile)
                        .join(mod_id)
                        .join(&entry.key),
                )
            }
            StorageArea::Trash => Some(self.data_dir.join("trash").join(&entry.key)),
        }
    }

    fn entry(
        &self,
        area: StorageArea,
        key: String,
        profile: Option<String>,
        path: PathBuf,
    ) -> StorageEntry {
        StorageEntry {
            area,
            key,
            profile_id: profile,
            is_link: is_link(&path),
            size_bytes: size_of(&path),
            path,
            group: None,
            created_at: None,
        }
    }
}

impl StorageInventoryPort for FilesystemStorageInventory {
    fn scan(&self) -> AppResult<Vec<StorageEntry>> {
        let mut entries = Vec::new();
        for (name, path) in children(&self.packages_dir) {
            if let Some(hash) = name.strip_suffix(".zip").filter(|stem| is_sha256(stem)) {
                entries.push(self.entry(StorageArea::Package, hash.to_string(), None, path));
            }
        }
        for (name, path) in children(&self.smapi_cache_dir) {
            entries.push(self.entry(StorageArea::SmapiCache, name, None, path));
        }
        for (profile, profile_dir) in children(&self.setups_dir()) {
            if profile.starts_with('.') || is_link(&profile_dir) {
                continue;
            }
            for area in [StorageArea::Staging, StorageArea::Recovery] {
                let Some(root) = self.operation_root(&profile, area) else {
                    continue;
                };
                for (name, path) in children(&root) {
                    entries.push(self.entry(area, name, Some(profile.clone()), path));
                }
            }
        }
        // Recovery data the manager made, each dated by its folder name.
        for (save, save_dir) in children(&self.data_dir.join("save-backups")) {
            if is_link(&save_dir) {
                continue;
            }
            for (stamp, path) in children(&save_dir) {
                if stamp.ends_with(".part") {
                    continue;
                }
                let mut entry = self.entry(StorageArea::SaveBackup, stamp.clone(), None, path);
                entry.group = Some(save.clone());
                entry.created_at = parse_stamp(&stamp, "%Y%m%dT%H%M%S%3f", 18);
                entries.push(entry);
            }
        }
        for (profile, profile_dir) in children(&self.data_dir.join("config-backups")) {
            if is_link(&profile_dir) {
                continue;
            }
            for (mod_id, mod_dir) in children(&profile_dir) {
                if is_link(&mod_dir) {
                    continue;
                }
                for (stamp, path) in children(&mod_dir) {
                    let mut entry = self.entry(
                        StorageArea::ConfigBackup,
                        stamp.clone(),
                        Some(profile.clone()),
                        path,
                    );
                    entry.group = Some(mod_id.clone());
                    entry.created_at = parse_stamp(&stamp, "%Y%m%dT%H%M%S%3f", 18);
                    entries.push(entry);
                }
            }
        }
        for (name, path) in children(&self.data_dir.join("trash")) {
            let mut entry = self.entry(StorageArea::Trash, name.clone(), None, path);
            entry.created_at = name
                .len()
                .checked_sub(15)
                .and_then(|start| parse_stamp(&name[start..], "%Y%m%dT%H%M%S", 15));
            entries.push(entry);
        }
        Ok(entries)
    }

    fn remove(&self, entry: &StorageEntry) -> AppResult<()> {
        let expected = self.expected_path(entry).ok_or_else(|| {
            AppError::validation(
                "CLEANUP_PATH_REJECTED",
                "That item is not inside the manager's storage",
            )
        })?;
        if expected != entry.path {
            return Err(AppError::validation(
                "CLEANUP_PATH_REJECTED",
                "That item is not inside the manager's storage",
            ));
        }
        // Every folder between the storage root and the item must be real, so
        // a link swapped in after the scan cannot redirect the deletion.
        let mut ancestor = expected.parent();
        while let Some(dir) = ancestor {
            if is_link(dir) {
                return Err(AppError::validation(
                    "CLEANUP_LINK_REFUSED",
                    "A folder on the way to that item is a link, so it was left alone",
                ));
            }
            if dir == self.data_dir || dir == self.smapi_cache_dir || dir == self.packages_dir {
                break;
            }
            ancestor = dir.parent();
        }
        let meta = match std::fs::symlink_metadata(&expected) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(AppError::filesystem(
                    "Could not read the item before removing it",
                    error.to_string(),
                ))
            }
        };
        if meta.file_type().is_symlink() {
            return Err(AppError::validation(
                "CLEANUP_LINK_REFUSED",
                "That item is a link, so it was left alone",
            ));
        }
        let result = if meta.is_dir() {
            fs::remove_dir_all(&expected)
        } else {
            fs::remove_file(&expected)
        };
        result.map_err(|error| AppError::filesystem("Could not remove the item", error.to_string()))
    }
}
