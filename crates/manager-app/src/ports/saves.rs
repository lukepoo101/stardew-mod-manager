use crate::error::AppResult;
use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// One Stardew Valley save folder, read without changing anything in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveInfo {
    /// The save folder's name (such as `Riverside_123456789`); stable across
    /// renames of the farm because the game never renames the folder.
    pub id: String,
    pub farm_name: Option<String>,
    pub farmer_name: Option<String>,
    pub game_version: Option<String>,
    pub modified_at: Option<DateTime<Utc>>,
    pub size_bytes: u64,
}

/// A verified copy of a save made by the manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveBackupInfo {
    /// `<save id>/<timestamp>`, unique per backup.
    pub id: String,
    pub save_id: String,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
    pub path: PathBuf,
}

/// Reads, backs up and restores Stardew Valley saves. Backups live in the
/// manager's own folder; the only change ever made to the game's save folder
/// is an explicitly requested restore, which first moves the live save aside.
pub trait SavesPort: Send + Sync {
    fn saves_dir(&self) -> Option<PathBuf>;
    fn list_saves(&self) -> AppResult<Vec<SaveInfo>>;
    /// Copies a save and verifies the copy matches before returning.
    fn backup(&self, save_id: &str) -> AppResult<SaveBackupInfo>;
    fn list_backups(&self) -> AppResult<Vec<SaveBackupInfo>>;
    /// Puts a backup back in place of the live save. The live save is kept as
    /// a backup of its own first, so a restore never loses anything.
    fn restore(&self, backup_id: &str) -> AppResult<SaveBackupInfo>;
}
