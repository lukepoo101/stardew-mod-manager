use crate::error::AppResult;
use std::path::PathBuf;

/// Which manager-owned area a stored item lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageArea {
    /// A retained package archive, named by its SHA-256.
    Package,
    /// A downloaded or extracted SMAPI installer.
    SmapiCache,
    /// Files an operation prepared before moving them into place.
    Staging,
    /// Copies an operation kept so it could undo itself.
    Recovery,
    /// A manager-made copy of a Stardew save (`<save>/<time>`).
    SaveBackup,
    /// A mod's settings saved before a replace or reinstall
    /// (`<profile>/<mod>/<time>`).
    ConfigBackup,
    /// A deleted profile's folder.
    Trash,
}

/// One top-level item inside a manager-owned area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageEntry {
    pub area: StorageArea,
    /// The package hash, operation id or cache item name.
    pub key: String,
    /// The profile whose folder holds a staging or recovery item.
    pub profile_id: Option<String>,
    pub path: PathBuf,
    /// Bytes on disk, counted without following links.
    pub size_bytes: u64,
    /// The item is itself a symbolic link or junction. Cleanup never removes
    /// or follows these.
    pub is_link: bool,
    /// Items that retention counts together, such as the backups of one save.
    pub group: Option<String>,
    /// When the item was made, read from its name where the manager wrote one.
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Lists and removes items in the manager's own storage.
///
/// Implementations only ever see the fixed manager-owned roots: the service
/// cannot ask for an arbitrary path to be deleted.
pub trait StorageInventoryPort: Send + Sync {
    fn scan(&self) -> AppResult<Vec<StorageEntry>>;
    /// Removes one scanned item. Implementations re-check that the path is a
    /// direct child of its area's root and is not a link before deleting.
    fn remove(&self, entry: &StorageEntry) -> AppResult<()>;
}
