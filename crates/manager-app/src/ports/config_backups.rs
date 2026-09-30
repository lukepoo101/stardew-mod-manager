use crate::error::AppResult;
use chrono::{DateTime, Utc};
use manager_core::ids::ProfileId;

/// A saved copy of one mod's settings files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigBackup {
    /// `<unique id>/<timestamp>`, unique per backup.
    pub id: String,
    pub unique_id: String,
    pub created_at: DateTime<Utc>,
    /// Relative paths of the files kept.
    pub files: Vec<String>,
}

/// Keeps mods' settings files apart from the mods themselves, per profile.
pub trait ConfigBackupsPort: Send + Sync {
    /// Saves the files and reads them back to check before returning.
    fn save(
        &self,
        profile_id: &ProfileId,
        unique_id: &str,
        files: &[(String, Vec<u8>)],
    ) -> AppResult<ConfigBackup>;
    fn list(&self, profile_id: &ProfileId, unique_id: &str) -> AppResult<Vec<ConfigBackup>>;
    fn load(&self, profile_id: &ProfileId, backup_id: &str) -> AppResult<Vec<(String, Vec<u8>)>>;
}
