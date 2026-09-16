use crate::dependency::DependencyReport;
use crate::manifest::Manifest;
use serde::{Deserialize, Serialize};

pub const MAX_COMPRESSED_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB
pub const MAX_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
pub const MAX_ENTRY_COUNT: usize = 20_000;

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
