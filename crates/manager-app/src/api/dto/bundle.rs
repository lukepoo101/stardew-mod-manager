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
    /// Mods whose settings were included.
    #[serde(default)]
    pub settings_included: Vec<String>,
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
    /// The author marked it optional: it is installed only if chosen.
    #[serde(default)]
    pub optional: bool,
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
    /// UniqueIDs of mods whose settings the bundle carries.
    #[serde(default)]
    pub settings_for: Vec<String>,
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
    /// Mods whose settings from the bundle were written.
    #[serde(default)]
    pub settings_applied: Vec<String>,
    /// Optional mods the bundle offered that were not chosen.
    #[serde(default)]
    pub declined_optional: Vec<String>,
    /// The bundle's mod list was kept as the new profile's group reference.
    #[serde(default)]
    pub reference_attached: bool,
}

/// A duplicate that was started but not finished.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "UnfinishedCopyDto.ts")]
pub struct UnfinishedCopyDto {
    pub profile_id: String,
    pub profile_name: String,
    /// The profile it is a copy of, by its name at the time.
    pub source_name: String,
    /// How many mods the finished copy should have.
    pub expected_mods: usize,
}
