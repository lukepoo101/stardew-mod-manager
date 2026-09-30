//! Mods' settings files kept under `config-backups/<profile>/<mod>/<time>/`.
#![allow(clippy::result_large_err)]

use crate::paths::AppPaths;
use chrono::Utc;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::config_backups::{ConfigBackup, ConfigBackupsPort};
use manager_core::ids::ProfileId;
use std::path::{Component, Path, PathBuf};

pub struct FilesystemConfigBackups {
    root: PathBuf,
}

fn plain(value: &str) -> bool {
    !value.is_empty()
        && Path::new(value)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn single(value: &str) -> bool {
    plain(value) && !value.contains(['/', '\\'])
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

impl FilesystemConfigBackups {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            root: paths.data_dir().join("config-backups"),
        }
    }

    fn mod_dir(&self, profile_id: &ProfileId, unique_id: &str) -> AppResult<PathBuf> {
        if !single(unique_id) {
            return Err(AppError::validation(
                "MOD_ID_INVALID",
                "That mod id cannot name a folder",
            ));
        }
        Ok(self
            .root
            .join(profile_id.to_string())
            .join(unique_id.to_lowercase()))
    }
}

impl ConfigBackupsPort for FilesystemConfigBackups {
    fn save(
        &self,
        profile_id: &ProfileId,
        unique_id: &str,
        files: &[(String, Vec<u8>)],
    ) -> AppResult<ConfigBackup> {
        let created_at = Utc::now();
        let stamp = created_at.format("%Y%m%dT%H%M%S%3f").to_string();
        let dir = self.mod_dir(profile_id, unique_id)?.join(&stamp);
        for (relative, bytes) in files {
            if !plain(relative) {
                return Err(AppError::validation(
                    "SETTINGS_PATH_INVALID",
                    "A settings file path is not inside the mod folder",
                ));
            }
            let target = dir.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AppError::filesystem("Could not back up the settings", e.to_string())
                })?;
            }
            std::fs::write(&target, bytes).map_err(|e| {
                AppError::filesystem("Could not back up the settings", e.to_string())
            })?;
            // Read it back: a backup that cannot be read is not a backup.
            if std::fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
                return Err(AppError::filesystem(
                    "The settings backup did not match",
                    target.display().to_string(),
                ));
            }
        }
        Ok(ConfigBackup {
            id: format!("{}/{stamp}", unique_id.to_lowercase()),
            unique_id: unique_id.to_string(),
            created_at,
            files: files.iter().map(|(path, _)| path.clone()).collect(),
        })
    }

    fn list(&self, profile_id: &ProfileId, unique_id: &str) -> AppResult<Vec<ConfigBackup>> {
        let dir = self.mod_dir(profile_id, unique_id)?;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let Ok(stamp) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&stamp, "%Y%m%dT%H%M%S%3f")
            else {
                continue;
            };
            let mut files = Vec::new();
            let _ = walk(&entry.path(), &entry.path(), &mut files);
            files.sort();
            out.push(ConfigBackup {
                id: format!("{}/{stamp}", unique_id.to_lowercase()),
                unique_id: unique_id.to_string(),
                created_at: naive.and_utc(),
                files,
            });
        }
        out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        Ok(out)
    }

    fn load(&self, profile_id: &ProfileId, backup_id: &str) -> AppResult<Vec<(String, Vec<u8>)>> {
        let (unique_id, stamp) = backup_id
            .split_once('/')
            .filter(|(u, s)| single(u) && single(s))
            .ok_or_else(|| {
                AppError::validation("BACKUP_ID_INVALID", "That is not a settings backup")
            })?;
        let dir = self.mod_dir(profile_id, unique_id)?.join(stamp);
        let mut files = Vec::new();
        walk(&dir, &dir, &mut files).map_err(|e| {
            AppError::filesystem("Could not read the settings backup", e.to_string())
        })?;
        files
            .into_iter()
            .map(|relative| {
                std::fs::read(dir.join(&relative))
                    .map(|bytes| (relative, bytes))
                    .map_err(|e| {
                        AppError::filesystem("Could not read the settings backup", e.to_string())
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_round_trip_and_reject_escapes() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
        let backups = FilesystemConfigBackups::new(&paths);
        let profile = ProfileId::new();
        let saved = backups
            .save(
                &profile,
                "A.Mod",
                &[("config.json".into(), b"{\"a\":1}".to_vec())],
            )
            .unwrap();
        let listed = backups.list(&profile, "a.mod").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].files, vec!["config.json"]);
        assert_eq!(
            backups.load(&profile, &saved.id).unwrap(),
            vec![("config.json".to_string(), b"{\"a\":1}".to_vec())]
        );
        assert!(backups.save(&profile, "../x", &[]).is_err());
        assert!(backups
            .save(&profile, "A.Mod", &[("../escape".into(), vec![])])
            .is_err());
        assert!(backups.load(&profile, "a.mod/../../x").is_err());
    }
}
