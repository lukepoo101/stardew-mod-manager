use crate::error::AppResult;
use manager_core::ids::ArtifactHash;
use std::path::{Path, PathBuf};

/// A package taken out of a bundle and verified against the name it had there.
pub struct BundledPackage {
    pub hash: ArtifactHash,
    pub path: PathBuf,
}

/// One settings file carried in a bundle for a mod, by UniqueID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledSetting {
    pub unique_id: String,
    /// Path inside the mod's folder; always a `config.json`.
    pub relative_path: String,
    pub bytes: Vec<u8>,
}

pub struct BundleContents {
    pub recipe_json: String,
    pub packages: Vec<BundledPackage>,
    pub settings: Vec<BundledSetting>,
}

/// What a bundle holds, read without extracting packages.
pub struct BundlePeek {
    pub recipe_json: String,
    pub packages: Vec<ArtifactHash>,
    /// UniqueIDs of the mods whose settings are included.
    pub settings_for: Vec<String>,
}

/// Reads and writes profile bundles: one archive holding a recipe and the
/// packages it names.
///
/// A bundle is untrusted input. Implementations accept only the recipe and
/// content-addressed package entries, bound every size, and verify each package
/// against the digest in its name before handing it back.
pub trait BundleArchivePort: Send + Sync {
    /// Writes a new archive in `dest_dir` and returns its path. An existing file
    /// is never overwritten, and an interrupted write never leaves a file that
    /// looks complete.
    fn write_bundle(
        &self,
        dest_dir: &Path,
        base_name: &str,
        recipe_json: &str,
        packages: &[(ArtifactHash, PathBuf)],
        settings: &[BundledSetting],
    ) -> AppResult<PathBuf>;

    /// Reads the recipe and the names of the packages without extracting them.
    fn peek_bundle(&self, path: &Path) -> AppResult<BundlePeek>;

    /// Extracts the packages into `extract_dir`, verifying each digest.
    fn read_bundle(&self, path: &Path, extract_dir: &Path) -> AppResult<BundleContents>;

    /// Removes a folder created by `read_bundle`. Failure is ignored: it is
    /// scratch space, and leaving it behind harms nothing.
    fn discard_scratch(&self, dir: &Path);
}
