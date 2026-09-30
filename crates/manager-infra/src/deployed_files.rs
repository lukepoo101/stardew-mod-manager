//! Reads the files of a deployed mod folder, for comparing with what was
//! installed. Never writes.
#![allow(clippy::result_large_err)]

use crate::paths::AppPaths;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::deployed_files::{DeployedFile, DeployedFilesPort};
use manager_core::ids::ProfileId;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path};

pub struct FilesystemDeployedFiles {
    paths: AppPaths,
}

impl FilesystemDeployedFiles {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }
}

fn sha256(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(manager_core::ids::hash_to_hex(hasher.finalize()))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<DeployedFile>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(DeployedFile {
                relative_path: relative.to_string_lossy().replace('\\', "/"),
                size_bytes: meta.len(),
                sha256: sha256(&path)?,
            });
        }
    }
    Ok(())
}

impl DeployedFilesPort for FilesystemDeployedFiles {
    fn read_folder(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
    ) -> AppResult<Option<Vec<DeployedFile>>> {
        let relative = Path::new(root_relative_path);
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(AppError::validation(
                "DEPLOYMENT_PATH_INVALID",
                "The recorded folder is not a plain relative path",
            ));
        }
        let candidates = [
            self.paths.profile_mods_dir(profile_id).join(relative),
            self.paths.profile_disabled_dir(profile_id).join(relative),
        ];
        let Some(folder) = candidates.iter().find(|p| p.is_dir()) else {
            return Ok(None);
        };
        let mut files = Vec::new();
        walk(folder, folder, &mut files)
            .map_err(|e| AppError::filesystem("Could not read the mod's files", e.to_string()))?;
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(Some(files))
    }
}
