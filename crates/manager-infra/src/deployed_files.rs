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

/// Every `config.json` under `folder`, with its path relative to it.
pub(crate) fn config_files_in(folder: &Path) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    walk(folder, folder, &mut files)?;
    let mut out = Vec::new();
    for file in files {
        let is_config = file
            .relative_path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("config.json"));
        if is_config {
            out.push((
                file.relative_path.clone(),
                std::fs::read(folder.join(&file.relative_path))?,
            ));
        }
    }
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<DeployedFile>) -> std::io::Result<()> {
    walk_with(root, dir, out, true)
}

/// Lists files with their sizes; contents are hashed only when `hash` is set.
fn walk_with(
    root: &Path,
    dir: &Path,
    out: &mut Vec<DeployedFile>,
    hash: bool,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk_with(root, &path, out, hash)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(DeployedFile {
                relative_path: relative.to_string_lossy().replace('\\', "/"),
                size_bytes: meta.len(),
                sha256: if hash { sha256(&path)? } else { String::new() },
            });
        }
    }
    Ok(())
}

impl FilesystemDeployedFiles {
    fn folder(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
    ) -> AppResult<Option<std::path::PathBuf>> {
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
        Ok([
            self.paths.profile_mods_dir(profile_id).join(relative),
            self.paths.profile_disabled_dir(profile_id).join(relative),
        ]
        .into_iter()
        .find(|p| p.is_dir()))
    }
}

fn safe_relative(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)))
}

impl DeployedFilesPort for FilesystemDeployedFiles {
    fn read_configs(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
    ) -> AppResult<Vec<(String, Vec<u8>)>> {
        let Some(folder) = self.folder(profile_id, root_relative_path)? else {
            return Ok(Vec::new());
        };
        let mut files = Vec::new();
        walk(&folder, &folder, &mut files)
            .map_err(|e| AppError::filesystem("Could not read the mod's files", e.to_string()))?;
        let mut out = Vec::new();
        for file in files {
            let is_config = file
                .relative_path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("config.json"));
            if is_config {
                let bytes = std::fs::read(folder.join(&file.relative_path)).map_err(|e| {
                    AppError::filesystem("Could not read a settings file", e.to_string())
                })?;
                out.push((file.relative_path, bytes));
            }
        }
        Ok(out)
    }

    fn write_files(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
        files: &[(String, Vec<u8>)],
    ) -> AppResult<()> {
        let Some(folder) = self.folder(profile_id, root_relative_path)? else {
            return Err(AppError::validation(
                "MOD_FILES_MISSING",
                "The mod's folder is not where the manager put it",
            ));
        };
        for (relative, bytes) in files {
            if !safe_relative(relative) {
                return Err(AppError::validation(
                    "SETTINGS_PATH_INVALID",
                    "A settings file path is not inside the mod folder",
                ));
            }
            let target = folder.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AppError::filesystem("Could not restore a settings file", e.to_string())
                })?;
            }
            std::fs::write(&target, bytes).map_err(|e| {
                AppError::filesystem("Could not restore a settings file", e.to_string())
            })?;
        }
        Ok(())
    }

    fn read_folder(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
    ) -> AppResult<Option<Vec<DeployedFile>>> {
        let Some(folder) = self.folder(profile_id, root_relative_path)? else {
            return Ok(None);
        };
        let mut files = Vec::new();
        walk(&folder, &folder, &mut files)
            .map_err(|e| AppError::filesystem("Could not read the mod's files", e.to_string()))?;
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(Some(files))
    }

    fn read_folder_sizes(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
    ) -> AppResult<Option<Vec<DeployedFile>>> {
        let Some(folder) = self.folder(profile_id, root_relative_path)? else {
            return Ok(None);
        };
        let mut files = Vec::new();
        walk_with(&folder, &folder, &mut files, false)
            .map_err(|e| AppError::filesystem("Could not read the mod's files", e.to_string()))?;
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(Some(files))
    }
}
