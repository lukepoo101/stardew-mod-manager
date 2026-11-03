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
    /// What was taken out of the included settings before they were
    /// written, one line per file, such as `Cloud Sync config.json: ...`.
    #[serde(default)]
    pub settings_redacted: Vec<String>,
    /// Settings files that could not be carried, with why.
    #[serde(default)]
    pub settings_skipped: Vec<String>,
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
    /// Operating systems the author says it works on; empty means all.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// It is marked for other operating systems, so it is never installed
    /// on this computer.
    #[serde(default)]
    pub not_for_this_computer: bool,
    /// Other mods in the recipe that come in the same package, which is
    /// installed (or left out) as a whole.
    #[serde(default)]
    pub shares_package_with: Vec<String>,
    /// Why it will not be installed although it is meant for this
    /// computer, such as sharing a package with a mod that is not.
    #[serde(default)]
    pub left_out_reason: Option<String>,
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
    /// Requirements no mod in the bundle provides, as "Mod needs Id".
    #[serde(default)]
    pub unresolved_requirements: Vec<String>,
    /// Mods whose files were changed outside the manager when exported.
    #[serde(default)]
    pub locally_modified: Vec<String>,
    /// When and why the exported profile was frozen, if it was.
    #[serde(default)]
    pub frozen_at: Option<String>,
    #[serde(default)]
    pub frozen_reason: Option<String>,
    /// This computer's operating system ("windows", "linux", "macos").
    #[serde(default)]
    pub operating_system: String,
    /// SHA-256 of the bundle file as read, sent back with the import so a
    /// file that changed since is refused rather than imported unseen.
    #[serde(default)]
    pub bundle_digest: String,
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
    /// Mods marked for other operating systems, which were not installed.
    #[serde(default)]
    pub not_for_this_computer: Vec<String>,
    /// Optional mods the recipient left unticked that come in one package
    /// with a mod that was installed. A package is installed, enabled and
    /// disabled as a whole, so they came with it.
    #[serde(default)]
    pub installed_with_package: Vec<String>,
}

/// How far a running bundle import has got, for showing while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BundleProgressDto.ts")]
pub struct BundleProgressDto {
    /// Packages handled so far.
    pub done: usize,
    pub total: usize,
    /// The mod or mods being installed now.
    pub current: String,
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

/// How one mod's settings files compare between two profiles. Values are
/// never included, only which files differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SettingsComparisonDto.ts")]
pub struct SettingsComparisonDto {
    pub unique_id: String,
    pub name: String,
    /// "same", "different", "only_first" or "only_second": whether the mod
    /// has settings files in both profiles and whether they match.
    pub state: String,
    /// Files whose contents differ, or that only one side has.
    pub files: Vec<String>,
}

/// A deleted profile whose folder is still in the trash, with its record.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "DeletedProfileDto.ts")]
pub struct DeletedProfileDto {
    /// The trash entry, used to bring it back.
    pub entry: String,
    pub name: String,
    pub deleted_at: String,
    pub mod_count: usize,
}
