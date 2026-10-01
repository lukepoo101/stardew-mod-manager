use crate::dependency::DependencyReport;
use crate::manifest::Manifest;
use serde::{Deserialize, Serialize};

pub const MAX_COMPRESSED_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB
pub const MAX_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
pub const MAX_ENTRY_COUNT: usize = 20_000;
/// Longest archive entry name accepted, in bytes. Windows path limits are the
/// tightest, and the entry is joined under a profile path that is already long.
pub const MAX_ENTRY_PATH_BYTES: usize = 240;
/// Archives that expand to less than this are never judged by their ratio, so
/// small, highly compressible mods (text, JSON) are not false positives.
pub const EXPANSION_RATIO_FLOOR_BYTES: u64 = 256 * 1024 * 1024;
/// Largest accepted ratio of uncompressed to compressed size above the floor.
pub const MAX_EXPANSION_RATIO: u64 = 100;

/// Rejects an archive whose declared expansion looks like a decompression bomb.
pub fn check_expansion(total_uncompressed: u64, compressed: u64) -> Result<(), String> {
    if total_uncompressed <= EXPANSION_RATIO_FLOOR_BYTES {
        return Ok(());
    }
    // A zero compressed size cannot be a real archive of this size.
    if compressed == 0 || total_uncompressed / compressed > MAX_EXPANSION_RATIO {
        return Err(format!(
            "Archive expands from {} to {} bytes, which exceeds the {}x compression ratio limit",
            compressed, total_uncompressed, MAX_EXPANSION_RATIO
        ));
    }
    Ok(())
}

#[cfg(test)]
mod expansion_tests {
    use super::*;

    #[test]
    fn small_archives_are_never_judged_by_ratio() {
        assert!(check_expansion(EXPANSION_RATIO_FLOOR_BYTES, 1).is_ok());
    }

    #[test]
    fn a_large_archive_within_the_ratio_is_accepted() {
        let compressed = 10 * 1024 * 1024;
        assert!(check_expansion(compressed * MAX_EXPANSION_RATIO, compressed).is_ok());
    }

    #[test]
    fn a_large_archive_beyond_the_ratio_is_rejected() {
        let error = check_expansion(EXPANSION_RATIO_FLOOR_BYTES * 2, 1024 * 1024).unwrap_err();
        assert!(error.contains("compression ratio"));
        assert!(check_expansion(EXPANSION_RATIO_FLOOR_BYTES * 2, 0).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InventoryEntryType {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InventoryEntry {
    pub relative_path: String,
    pub entry_type: InventoryEntryType,
    pub size_bytes: u64,
    pub sha256_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentManifest {
    pub manifest: Manifest,
    pub raw_manifest: String,
    pub relative_subfolder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    pub plan_id: String,
    pub setup_id: String,
    pub package_hash: String,
    pub original_filename: String,
    pub mod_folder_name: String,
    pub manifest: Manifest,
    pub raw_manifest: String,
    pub file_inventory: Vec<String>,
    #[serde(default)]
    pub trusted_inventory: Vec<InventoryEntry>,
    pub dependency_report: DependencyReport,
    #[serde(default)]
    pub component_manifests: Vec<ComponentManifest>,
    /// Files in the archive outside every component folder, such as a
    /// readme next to the mod's folder. They stay in the stored archive and
    /// are not installed.
    #[serde(default)]
    pub not_installed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovalPlan {
    pub operation_id: String,
    pub setup_id: String,
    pub installed_mod_id: String,
    pub mod_unique_id: String,
    pub relative_folder_path: String,
    pub recovery_folder_path: String,
    #[serde(default)]
    pub bundle_mod_ids: Vec<String>,
    #[serde(default)]
    pub bundle_root_folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveInspectionResult {
    pub selection_id: String,
    pub package_hash: String,
    pub original_filename: String,
    pub byte_size: u64,
    pub plan: InstallPlan,
}

/// Validate an internal relative path before joining it to a managed root.
pub fn validate_relative_path(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.contains('\\')
        || value.contains(':')
        || value.bytes().any(|b| b == 0 || b.is_ascii_control())
        || value
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        || std::path::Path::new(value).is_absolute()
    {
        return Err("Invalid managed path".into());
    }
    Ok(())
}
