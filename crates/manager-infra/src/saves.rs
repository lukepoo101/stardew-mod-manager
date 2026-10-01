//! Stardew Valley saves on disk, and the manager's verified copies of them.
#![allow(clippy::result_large_err)]

use crate::platform::shared::fs;
use chrono::{DateTime, Utc};
use manager_app::error::{AppError, AppResult};
use manager_app::ports::saves::{SaveBackupInfo, SaveInfo, SavesPort};
use std::path::{Path, PathBuf};

pub struct FilesystemSaves {
    saves_dir: Option<PathBuf>,
    backups_dir: PathBuf,
}

/// Where the game keeps saves on this platform.
pub fn default_saves_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.join("StardewValley").join("Saves"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| !p.as_os_str().is_empty())
                    .map(|home| home.join(".config"))
            })
            .map(|config| config.join("StardewValley").join("Saves"))
    }
}

/// A save id is a folder name: no separators, no parent references.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains(['/', '\\']) && !id.starts_with('.')
}

fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    let value = xml[start..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Files below `dir` as (relative path, size), never following links.
fn files(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
            } else if let Ok(relative) = path.strip_prefix(dir) {
                out.push((relative.to_path_buf(), meta.len()));
            }
        }
    }
    out.sort();
    out
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    for (relative, _) in files(from) {
        let target = to.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy_file(&from.join(&relative), &target)?;
    }
    Ok(())
}

impl FilesystemSaves {
    pub fn new(saves_dir: Option<PathBuf>, backups_dir: PathBuf) -> Self {
        Self {
            saves_dir,
            backups_dir,
        }
    }

    fn require_saves_dir(&self) -> AppResult<&Path> {
        self.saves_dir.as_deref().ok_or_else(|| {
            AppError::validation(
                "SAVES_DIR_UNKNOWN",
                "The Stardew Valley save folder could not be located",
            )
        })
    }

    fn save_path(&self, save_id: &str) -> AppResult<PathBuf> {
        if !valid_id(save_id) {
            return Err(AppError::validation(
                "SAVE_ID_INVALID",
                "That is not a save",
            ));
        }
        let path = self.require_saves_dir()?.join(save_id);
        if !path.is_dir() {
            return Err(AppError::validation(
                "SAVE_NOT_FOUND",
                "That save is not in the Stardew Valley save folder",
            ));
        }
        Ok(path)
    }

    /// Copies `source` into a new timestamped backup of `save_id` and checks
    /// every file arrived with the same size.
    fn copy_to_backup(
        &self,
        save_id: &str,
        source: &Path,
        label: &str,
    ) -> AppResult<SaveBackupInfo> {
        let created_at = Utc::now();
        let stamp = format!("{}{}", created_at.format("%Y%m%dT%H%M%S%3f"), label);
        let target = self.backups_dir.join(save_id).join(&stamp);
        let partial = self.backups_dir.join(save_id).join(format!("{stamp}.part"));
        let needed: u64 = files(source).iter().map(|(_, size)| size).sum();
        crate::free_space::ensure(
            &self.backups_dir,
            needed,
            "this save backup",
            "the manager's backup folder",
        )
        .map_err(|message| AppError::validation("NOT_ENOUGH_SPACE", message))?;
        copy_tree(source, &partial)
            .map_err(|e| AppError::filesystem("Could not copy the save", e.to_string()))?;
        if files(source) != files(&partial) {
            let _ = fs::remove_dir_all(&partial);
            return Err(AppError::filesystem(
                "The save copy did not match the original",
                "file list or sizes differ; the backup was discarded",
            ));
        }
        fs::rename_path(&partial, &target)
            .map_err(|e| AppError::filesystem("Could not finish the backup", e.to_string()))?;
        Ok(SaveBackupInfo {
            id: format!("{save_id}/{stamp}"),
            save_id: save_id.to_string(),
            created_at,
            size_bytes: files(&target).iter().map(|(_, size)| size).sum(),
            path: target,
        })
    }
}

impl SavesPort for FilesystemSaves {
    fn saves_dir(&self) -> Option<PathBuf> {
        self.saves_dir.clone()
    }

    fn list_saves(&self) -> AppResult<Vec<SaveInfo>> {
        let Some(dir) = &self.saves_dir else {
            return Ok(Vec::new());
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(Vec::new());
        };
        let mut saves = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let Ok(id) = entry.file_name().into_string() else {
                continue;
            };
            let path = entry.path();
            if !valid_id(&id) || !path.is_dir() {
                continue;
            }
            let info = std::fs::read_to_string(path.join("SaveGameInfo")).unwrap_or_default();
            // A folder without SaveGameInfo is not a save the game can load.
            if info.is_empty() {
                continue;
            }
            let modified_at = std::fs::metadata(path.join("SaveGameInfo"))
                .and_then(|m| m.modified())
                .ok()
                .map(DateTime::<Utc>::from);
            saves.push(SaveInfo {
                farm_name: tag(&info, "farmName"),
                farmer_name: tag(&info, "name"),
                game_version: tag(&info, "gameVersion"),
                modified_at,
                size_bytes: files(&path).iter().map(|(_, size)| size).sum(),
                id,
            });
        }
        saves.sort_by(|a, b| b.modified_at.cmp(&a.modified_at).then(a.id.cmp(&b.id)));
        Ok(saves)
    }

    fn backup(&self, save_id: &str) -> AppResult<SaveBackupInfo> {
        let source = self.save_path(save_id)?;
        self.copy_to_backup(save_id, &source, "")
    }

    fn list_backups(&self) -> AppResult<Vec<SaveBackupInfo>> {
        let mut out = Vec::new();
        let Ok(saves) = std::fs::read_dir(&self.backups_dir) else {
            return Ok(out);
        };
        for save in saves.filter_map(Result::ok) {
            let Ok(save_id) = save.file_name().into_string() else {
                continue;
            };
            let Ok(stamps) = std::fs::read_dir(save.path()) else {
                continue;
            };
            for stamp in stamps.filter_map(Result::ok) {
                let Ok(name) = stamp.file_name().into_string() else {
                    continue;
                };
                if name.ends_with(".part") || !stamp.path().is_dir() {
                    continue;
                }
                let created_at = chrono::NaiveDateTime::parse_from_str(
                    name.get(..18).unwrap_or(""),
                    "%Y%m%dT%H%M%S%3f",
                )
                .map(|naive| naive.and_utc())
                .unwrap_or_else(|_| Utc::now());
                out.push(SaveBackupInfo {
                    id: format!("{save_id}/{name}"),
                    save_id: save_id.clone(),
                    created_at,
                    size_bytes: files(&stamp.path()).iter().map(|(_, size)| size).sum(),
                    path: stamp.path(),
                });
            }
        }
        out.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        Ok(out)
    }

    fn restore(&self, backup_id: &str) -> AppResult<SaveBackupInfo> {
        let (save_id, stamp) = backup_id.split_once('/').ok_or_else(|| {
            AppError::validation("BACKUP_ID_INVALID", "That is not a save backup")
        })?;
        if !valid_id(save_id) || !valid_id(stamp) || stamp.ends_with(".part") {
            return Err(AppError::validation(
                "BACKUP_ID_INVALID",
                "That is not a save backup",
            ));
        }
        let backup = self.backups_dir.join(save_id).join(stamp);
        if !backup.is_dir() {
            return Err(AppError::validation(
                "BACKUP_NOT_FOUND",
                "That backup no longer exists",
            ));
        }
        let live = self.require_saves_dir()?.join(save_id);
        // Keep the current live save first, so the restore loses nothing.
        let kept = if live.is_dir() {
            Some(self.copy_to_backup(save_id, &live, "-before-restore")?)
        } else {
            None
        };
        let incoming = self
            .require_saves_dir()?
            .join(format!(".{save_id}.restoring"));
        let _ = fs::remove_dir_all(&incoming);
        copy_tree(&backup, &incoming)
            .map_err(|e| AppError::filesystem("Could not copy the backup", e.to_string()))?;
        if files(&backup) != files(&incoming) {
            let _ = fs::remove_dir_all(&incoming);
            return Err(AppError::filesystem(
                "The restored copy did not match the backup",
                "nothing was changed",
            ));
        }
        if live.is_dir() {
            fs::remove_dir_all(&live).map_err(|e| {
                AppError::filesystem("Could not replace the live save", e.to_string())
            })?;
        }
        fs::rename_path(&incoming, &live).map_err(|e| {
            AppError::filesystem("Could not put the backup in place", e.to_string())
        })?;
        Ok(kept.unwrap_or(SaveBackupInfo {
            id: backup_id.to_string(),
            save_id: save_id.to_string(),
            created_at: Utc::now(),
            size_bytes: 0,
            path: backup,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(dir: &Path, id: &str, farm: &str) {
        let path = dir.join(id);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("SaveGameInfo"),
            format!("<Farmer><name>Abigail Fan</name><farmName>{farm}</farmName><gameVersion>1.6.15</gameVersion></Farmer>"),
        )
        .unwrap();
        std::fs::write(path.join(id), "world state").unwrap();
    }

    fn fixture() -> (tempfile::TempDir, FilesystemSaves) {
        let tmp = tempfile::tempdir().unwrap();
        let saves = tmp.path().join("Saves");
        save(&saves, "Riverside_123", "Riverside");
        std::fs::create_dir_all(saves.join("not_a_save")).unwrap();
        let adapter = FilesystemSaves::new(Some(saves), tmp.path().join("backups"));
        (tmp, adapter)
    }

    #[test]
    fn lists_saves_from_their_info_file_only() {
        let (_tmp, adapter) = fixture();
        let saves = adapter.list_saves().unwrap();
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].id, "Riverside_123");
        assert_eq!(saves[0].farm_name.as_deref(), Some("Riverside"));
        assert_eq!(saves[0].farmer_name.as_deref(), Some("Abigail Fan"));
        assert_eq!(saves[0].game_version.as_deref(), Some("1.6.15"));
    }

    #[test]
    fn a_backup_is_a_verified_copy_and_leaves_the_save_alone() {
        let (tmp, adapter) = fixture();
        let before = std::fs::read(tmp.path().join("Saves/Riverside_123/Riverside_123")).unwrap();
        let backup = adapter.backup("Riverside_123").unwrap();
        assert!(backup.path.join("Riverside_123").exists());
        assert_eq!(
            files(&backup.path),
            files(&tmp.path().join("Saves/Riverside_123"))
        );
        assert_eq!(
            std::fs::read(tmp.path().join("Saves/Riverside_123/Riverside_123")).unwrap(),
            before
        );
        assert_eq!(adapter.list_backups().unwrap().len(), 1);
        assert!(adapter.backup("../escape").is_err());
        assert!(adapter.backup("Missing_1").is_err());
    }

    #[test]
    fn restoring_keeps_the_current_save_as_a_backup_first() {
        let (tmp, adapter) = fixture();
        let live = tmp.path().join("Saves/Riverside_123/Riverside_123");
        let backup = adapter.backup("Riverside_123").unwrap();
        std::fs::write(&live, "a later, worse day").unwrap();

        let kept = adapter.restore(&backup.id).unwrap();
        assert_eq!(std::fs::read_to_string(&live).unwrap(), "world state");
        assert_eq!(
            std::fs::read_to_string(kept.path.join("Riverside_123")).unwrap(),
            "a later, worse day"
        );
        assert_eq!(adapter.list_backups().unwrap().len(), 2);
        assert!(adapter.restore("Riverside_123/../../x").is_err());
    }
}
