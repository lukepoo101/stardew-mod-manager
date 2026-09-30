//! Measures how much space the manager's folders use, without following links.

use crate::paths::AppPaths;
use manager_app::ports::storage_usage::{AreaUsage, ProfileUsage, Size, StorageUsagePort};
use manager_core::ids::ProfileId;
use std::path::Path;

pub struct FilesystemStorageUsage {
    paths: AppPaths,
}

impl FilesystemStorageUsage {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }
}

/// A missing folder holds nothing; one that cannot be read is unknown.
fn size(path: &Path) -> Size {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(0),
        Err(_) => return None,
    };
    if meta.file_type().is_symlink() {
        return Some(0);
    }
    if !meta.is_dir() {
        return Some(meta.len());
    }
    let mut total = 0u64;
    for entry in std::fs::read_dir(path).ok()? {
        total += size(&entry.ok()?.path())?;
    }
    Some(total)
}

fn sum(parts: [Size; 2]) -> Size {
    Some(parts[0]? + parts[1]?)
}

impl StorageUsagePort for FilesystemStorageUsage {
    fn profile(&self, profile_id: &ProfileId) -> ProfileUsage {
        let dir = self.paths.profile_dir(profile_id);
        ProfileUsage {
            live: size(&self.paths.profile_mods_dir(profile_id)),
            disabled: size(&self.paths.profile_disabled_dir(profile_id)),
            operations: sum([size(&dir.join(".staging")), size(&dir.join(".recovery"))]),
        }
    }

    fn areas(&self) -> AreaUsage {
        let data = self.paths.data_dir();
        AreaUsage {
            packages: size(&self.paths.packages_dir()),
            installer_cache: size(&self.paths.smapi_cache_dir()),
            save_backups: size(&data.join("save-backups")),
            trash: size(&data.join("trash")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_count_files_and_treat_missing_folders_as_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
        let profile = ProfileId::new();
        let mods = paths.profile_mods_dir(&profile).join("A");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("a.dll"), vec![0u8; 100]).unwrap();
        let usage = FilesystemStorageUsage::new(paths.clone());
        let profile_usage = usage.profile(&profile);
        assert_eq!(profile_usage.live, Some(100));
        assert_eq!(profile_usage.disabled, Some(0));
        assert_eq!(profile_usage.operations, Some(0));
        assert_eq!(usage.areas().trash, Some(0));
    }
}
