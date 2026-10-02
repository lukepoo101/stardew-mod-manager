use crate::error::AppResult;
use manager_core::ids::ProfileId;
use std::path::PathBuf;

/// Moves a profile's own folder aside instead of deleting it, so a deletion
/// that fails half way can be put back.
pub trait ProfileFolderPort: Send + Sync {
    /// Bytes in the profile's folder, not following links.
    fn folder_size(&self, profile_id: &ProfileId) -> u64;
    /// Moves the folder into the manager's trash. Returns where it went, or
    /// None when the profile never had a folder.
    fn move_to_trash(&self, profile_id: &ProfileId) -> AppResult<Option<PathBuf>>;
    /// Moves a trashed folder back to the profile's place.
    fn restore_from_trash(
        &self,
        profile_id: &ProfileId,
        trashed: &std::path::Path,
    ) -> AppResult<()>;
    /// Writes the record that lets a deleted profile be brought back, into
    /// the profile's folder so it travels with it to the trash.
    fn write_deletion_record(&self, profile_id: &ProfileId, json: &str) -> AppResult<()>;
    /// Deleted profiles still in the trash with a record: (trash entry, record).
    fn list_deletion_records(&self) -> AppResult<Vec<(String, String)>>;
    /// The settings files of one mod folder inside a trashed profile.
    fn trashed_configs(&self, entry: &str, folder: &str) -> AppResult<Vec<(String, Vec<u8>)>>;
    /// Removes the record once the profile has been brought back.
    fn forget_deletion_record(&self, entry: &str) -> AppResult<()>;
}

/// The file a deleted profile's record is kept in.
pub const DELETION_RECORD: &str = "deleted-profile.json";
