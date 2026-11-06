//! Stardew Valley saves on disk, and the manager's verified copies of them.
#![allow(clippy::result_large_err)]

use crate::platform::shared::fs;
use chrono::{DateTime, Utc};
use manager_app::error::{AppError, AppResult};
use manager_app::ports::saves::{SaveBackupInfo, SaveInfo, SavesPort};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The checksums of a backup's files, recorded when it is made and kept in
/// the backup's folder. Never copied back into a save.
const CHECKSUMS: &str = ".smm-backup-checksums.json";

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

/// Turns the game's XML escapes (`&amp;`, `&lt;`, `&#233;` and so on) back
/// into the characters the player typed. Anything unrecognised stays as is.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let entity = &rest[1..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    let value = unescape(xml[start..end].trim());
    (!value.is_empty()).then_some(value)
}

/// Files below `dir` as (relative path, size), never following links. A
/// backup's own checksum record is not one of the save's files.
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
                if relative != Path::new(CHECKSUMS) {
                    out.push((relative.to_path_buf(), meta.len()));
                }
            }
        }
    }
    out.sort();
    out
}

/// Each file's size and SHA-256, keyed by its path with `/` separators.
type Checksums = BTreeMap<String, (u64, String)>;

fn checksums(dir: &Path) -> std::io::Result<Checksums> {
    let mut out = BTreeMap::new();
    for (relative, size) in files(dir) {
        let mut hasher = Sha256::new();
        let mut file = std::fs::File::open(fs::extended_path(&dir.join(&relative)))?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = std::io::Read::read(&mut file, &mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let key = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        out.insert(
            key,
            (size, manager_core::ids::hash_to_hex(hasher.finalize())),
        );
    }
    Ok(out)
}

/// The files that differ between what was recorded and what is there now.
fn mismatches(expected: &Checksums, found: &Checksums) -> Vec<String> {
    expected
        .keys()
        .chain(found.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|path| expected.get(*path) != found.get(*path))
        .cloned()
        .collect()
}

/// Copies every file and keeps its modification time: the game and the
/// manager read "last played" from it, so a copy must not look newer.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    for (relative, _) in files(from) {
        let source = from.join(&relative);
        let target = to.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy_file(&source, &target)?;
        let modified = std::fs::symlink_metadata(fs::extended_path(&source))?.modified()?;
        std::fs::File::options()
            .write(true)
            .open(fs::extended_path(&target))?
            .set_modified(modified)?;
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

    /// Copies `source` into a new timestamped backup of `save_id`, checks
    /// every file arrived with the same content, and records each file's
    /// checksum with the backup so a restore can tell it is unchanged.
    fn copy_to_backup(
        &self,
        save_id: &str,
        source: &Path,
        label: &str,
    ) -> AppResult<SaveBackupInfo> {
        let created_at = Utc::now();
        let base = format!("{}{}", created_at.format("%Y%m%dT%H%M%S%3f"), label);
        // Two backups in the same millisecond would otherwise share a folder
        // and the second would fail to finish; a counter keeps both. The
        // date is still the first 18 characters, which is all listing reads.
        let folder = self.backups_dir.join(save_id);
        let mut stamp = base.clone();
        let mut n = 2;
        while folder.join(&stamp).exists() || folder.join(format!("{stamp}.part")).exists() {
            stamp = format!("{base}-{n}");
            n += 1;
        }
        let target = folder.join(&stamp);
        let partial = folder.join(format!("{stamp}.part"));
        let needed: u64 = files(source).iter().map(|(_, size)| size).sum();
        crate::free_space::ensure(
            &self.backups_dir,
            needed,
            "this save backup",
            "the manager's backup folder",
        )
        .map_err(|message| AppError::validation("NOT_ENOUGH_SPACE", message))?;
        let copied = checksums(source).and_then(|expected| {
            copy_tree(source, &partial)?;
            Ok((expected, checksums(&partial)?))
        });
        let (expected, found) = copied.map_err(|e| {
            let _ = fs::remove_dir_all(&partial);
            AppError::filesystem("Could not copy the save", e.to_string())
        })?;
        let differ = mismatches(&expected, &found);
        if !differ.is_empty() {
            let _ = fs::remove_dir_all(&partial);
            return Err(AppError::filesystem(
                "The save copy did not match the original",
                format!("{} differ; the backup was discarded", differ.join(", ")),
            ));
        }
        let record = serde_json::to_vec(&expected)
            .map_err(|e| AppError::internal("Could not record the backup", e.to_string()))?;
        std::fs::write(fs::extended_path(&partial.join(CHECKSUMS)), record).map_err(|e| {
            let _ = fs::remove_dir_all(&partial);
            AppError::filesystem("Could not record the backup's checksums", e.to_string())
        })?;
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

    /// Checks a backup's files against the checksums recorded when it was
    /// made. Backups made before checksums were recorded have none to check.
    fn verify_backup(&self, backup: &Path) -> AppResult<Option<Checksums>> {
        let record = match std::fs::read(fs::extended_path(&backup.join(CHECKSUMS))) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(AppError::filesystem(
                    "Could not read the backup's checksums",
                    e.to_string(),
                ))
            }
        };
        let damaged = |details: String| {
            AppError::validation(
                "SAVE_BACKUP_DAMAGED",
                "This backup has changed since it was made, so it was not restored and your current save was not touched. Choose another backup.",
            )
            .with_details(details)
        };
        let expected: Checksums = serde_json::from_slice(&record)
            .map_err(|e| damaged(format!("checksum record unreadable: {e}")))?;
        let found = checksums(backup)
            .map_err(|e| AppError::filesystem("Could not read the backup", e.to_string()))?;
        let differ = mismatches(&expected, &found);
        if !differ.is_empty() {
            return Err(damaged(format!(
                "differs from when it was made: {}",
                differ.join(", ")
            )));
        }
        Ok(Some(expected))
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
            // A folder without SaveGameInfo is not a save the game can load.
            let Ok(bytes) = std::fs::read(path.join("SaveGameInfo")) else {
                continue;
            };
            // An empty or unreadable one is a damaged save, which is listed
            // by its folder name: that is when a backup matters most.
            let info = String::from_utf8(bytes).unwrap_or_default();
            let farm_name = tag(&info, "farmName");
            let farmer_name = tag(&info, "name");
            let modified_at = std::fs::metadata(path.join("SaveGameInfo"))
                .and_then(|m| m.modified())
                .ok()
                .map(DateTime::<Utc>::from);
            saves.push(SaveInfo {
                info_unreadable: farm_name.is_none() && farmer_name.is_none(),
                farm_name,
                farmer_name,
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
        // A backup that is no longer what was saved is refused before
        // anything is copied, so a damaged one never replaces a good save.
        let recorded = self.verify_backup(&backup)?;
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
        let copied = copy_tree(&backup, &incoming).and_then(|()| {
            let expected = match recorded {
                Some(recorded) => recorded,
                None => checksums(&backup)?,
            };
            Ok(mismatches(&expected, &checksums(&incoming)?))
        });
        match copied {
            Ok(differ) if differ.is_empty() => {}
            Ok(differ) => {
                let _ = fs::remove_dir_all(&incoming);
                return Err(AppError::filesystem(
                    "The restored copy did not match the backup",
                    format!("{} differ; nothing was changed", differ.join(", ")),
                ));
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&incoming);
                return Err(AppError::filesystem(
                    "Could not copy the backup",
                    e.to_string(),
                ));
            }
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

    #[test]
    fn a_backup_damaged_after_it_was_made_is_refused_and_the_save_is_untouched() {
        let (tmp, adapter) = fixture();
        let live = tmp.path().join("Saves/Riverside_123/Riverside_123");
        let backup = adapter.backup("Riverside_123").unwrap();
        std::fs::write(&live, "today's farm").unwrap();
        // Same size, different bytes: a size check alone would pass this.
        std::fs::write(backup.path.join("Riverside_123"), "world STATE").unwrap();

        let error = adapter.restore(&backup.id).unwrap_err();
        assert_eq!(error.code, "SAVE_BACKUP_DAMAGED");
        assert!(error.summary.contains("not restored"), "{}", error.summary);
        assert!(error
            .technical_details
            .as_deref()
            .unwrap_or_default()
            .contains("Riverside_123"));
        assert_eq!(std::fs::read_to_string(&live).unwrap(), "today's farm");
        // Nothing was kept or copied, because nothing was about to change.
        assert_eq!(adapter.list_backups().unwrap().len(), 1);
        assert!(!tmp.path().join("Saves/.Riverside_123.restoring").exists());

        // A missing or extra file is caught the same way.
        let second = adapter.backup("Riverside_123").unwrap();
        std::fs::remove_file(second.path.join("SaveGameInfo")).unwrap();
        assert_eq!(
            adapter.restore(&second.id).unwrap_err().code,
            "SAVE_BACKUP_DAMAGED"
        );
    }

    #[test]
    fn backups_made_in_the_same_millisecond_are_all_kept() {
        let (_tmp, adapter) = fixture();
        let ids: std::collections::HashSet<String> = (0..20)
            .map(|_| adapter.backup("Riverside_123").unwrap().id)
            .collect();
        assert_eq!(ids.len(), 20);
        assert_eq!(adapter.list_backups().unwrap().len(), 20);
    }

    #[test]
    fn the_checksum_record_is_never_copied_into_the_save() {
        let (tmp, adapter) = fixture();
        let backup = adapter.backup("Riverside_123").unwrap();
        assert!(backup.path.join(CHECKSUMS).exists());
        adapter.restore(&backup.id).unwrap();
        assert!(!tmp
            .path()
            .join("Saves/Riverside_123")
            .join(CHECKSUMS)
            .exists());
        let size: u64 = files(&tmp.path().join("Saves/Riverside_123"))
            .iter()
            .map(|(_, s)| s)
            .sum();
        assert_eq!(backup.size_bytes, size);
    }

    #[test]
    fn backups_and_restores_keep_when_the_save_was_last_played() {
        let (tmp, adapter) = fixture();
        let info = tmp.path().join("Saves/Riverside_123/SaveGameInfo");
        let played =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options()
            .write(true)
            .open(&info)
            .unwrap()
            .set_modified(played)
            .unwrap();
        let before = adapter.list_saves().unwrap()[0].modified_at;

        let backup = adapter.backup("Riverside_123").unwrap();
        assert_eq!(
            std::fs::metadata(backup.path.join("SaveGameInfo"))
                .unwrap()
                .modified()
                .unwrap(),
            played
        );
        adapter.restore(&backup.id).unwrap();
        assert_eq!(adapter.list_saves().unwrap()[0].modified_at, before);
    }

    #[test]
    fn names_are_unescaped_and_damaged_saves_are_still_listed() {
        let (tmp, adapter) = fixture();
        let saves = tmp.path().join("Saves");
        let escaped = saves.join("Fish_1");
        std::fs::create_dir_all(&escaped).unwrap();
        std::fs::write(
            escaped.join("SaveGameInfo"),
            "<Farmer><name>Tom &amp; Jerry</name><farmName>Fish &lt;3 &#233;</farmName></Farmer>",
        )
        .unwrap();
        for (id, bytes) in [("Empty_2", &b""[..]), ("Binary_3", &[0xff, 0xfe, 0x00][..])] {
            std::fs::create_dir_all(saves.join(id)).unwrap();
            std::fs::write(saves.join(id).join("SaveGameInfo"), bytes).unwrap();
        }
        // No SaveGameInfo at all: not a save.
        std::fs::create_dir_all(saves.join("Folder_4")).unwrap();

        let listed = adapter.list_saves().unwrap();
        let by_id = |id: &str| listed.iter().find(|s| s.id == id);
        let fish = by_id("Fish_1").unwrap();
        assert_eq!(fish.farmer_name.as_deref(), Some("Tom & Jerry"));
        assert_eq!(fish.farm_name.as_deref(), Some("Fish <3 é"));
        assert!(!fish.info_unreadable);
        assert!(by_id("Empty_2").unwrap().info_unreadable);
        assert!(by_id("Binary_3").unwrap().info_unreadable);
        assert!(by_id("Folder_4").is_none());
        assert!(!by_id("Riverside_123").unwrap().info_unreadable);
        assert_eq!(unescape("a &unknown; & b &#xZZ;"), "a &unknown; & b &#xZZ;");
    }
}
