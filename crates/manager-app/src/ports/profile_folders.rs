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
}
