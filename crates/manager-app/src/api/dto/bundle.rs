use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BundleExportDto.ts")]
pub struct BundleExportDto {
    /// Where the bundle was written.
    pub path: String,
    pub component_count: usize,
    pub package_count: usize,
    /// Mods whose package is no longer stored, so the bundle cannot carry them.
    pub missing_packages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BundleComponentDto.ts")]
pub struct BundleComponentDto {
    pub unique_id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    /// Whether the bundle contains the package needed to install it.
    pub package_included: bool,
}

/// What a bundle contains, read without installing anything.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BundlePreviewDto.ts")]
pub struct BundlePreviewDto {
    pub profile_name: String,
    pub generated_at: String,
    pub components: Vec<BundleComponentDto>,
    /// Names of mods the bundle lists but does not carry.
    pub missing_packages: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BundleFailureDto.ts")]
pub struct BundleFailureDto {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BundleImportDto.ts")]
pub struct BundleImportDto {
    pub profile_id: String,
    pub profile_name: String,
    pub installed: Vec<String>,
    /// Installed but left disabled, as in the original profile.
    pub disabled: Vec<String>,
    pub failures: Vec<BundleFailureDto>,
}
