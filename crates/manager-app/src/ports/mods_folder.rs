//! Reading a Mods folder the manager does not own, for adoption.

use crate::error::AppResult;
use std::path::{Path, PathBuf};

/// One manifest found inside a mod folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedManifest {
    /// Path of the manifest relative to the mod folder.
    pub path: String,
    pub unique_id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub update_keys: Vec<String>,
    /// UniqueIDs it requires (its content-pack host first).
    pub requires: Vec<String>,
}

/// A top-level folder that holds at least one readable manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedMod {
    pub folder: String,
    pub manifests: Vec<ScannedManifest>,
    pub size_bytes: u64,
    pub file_count: usize,
    pub has_settings: bool,
    /// Things worth knowing before adopting it: unreadable manifests,
    /// links skipped, very deep layouts.
    pub problems: Vec<String>,
}

/// Something in the Mods folder that is not a recognisable mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedUnknown {
    pub name: String,
    pub size_bytes: u64,
    pub is_folder: bool,
    /// Why it is not treated as a mod.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderScan {
    pub mods_dir: PathBuf,
    pub mods: Vec<ScannedMod>,
    pub unknown: Vec<ScannedUnknown>,
    /// Folders SMAPI itself ships (not user mods).
    pub runtime: Vec<String>,
    /// Signs another mod manager deployed these files.
    pub manager_markers: Vec<String>,
    /// Changes when anything scanned changes, to revalidate a reviewed plan.
    pub fingerprint: String,
}

/// Reads a Mods folder without changing it, and packages one mod folder as
/// an archive for the normal install pipeline. Never runs mod code, never
/// follows links out of the folder.
pub trait ModsFolderPort: Send + Sync {
    fn scan(&self, mods_dir: &Path) -> AppResult<FolderScan>;
    /// Writes `folder` (a top-level entry of `mods_dir`) as a zip archive in
    /// `dest_dir`, with the folder as its root. Links are left out.
    fn package(&self, mods_dir: &Path, folder: &str, dest_dir: &Path) -> AppResult<PathBuf>;
    /// Files in `folder` whose contents differ from, or are missing in, the
    /// mod folder inside `archive` (settings files aside). Empty when they
    /// match. Compared by path below the mod's own folder.
    fn differences_from_archive(
        &self,
        mods_dir: &Path,
        folder: &str,
        archive: &Path,
    ) -> AppResult<Vec<String>>;
    /// Removes a folder of archives `package` wrote, once they are installed.
    fn discard(&self, work_dir: &Path);
}
