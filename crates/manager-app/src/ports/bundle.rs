use crate::error::AppResult;
use manager_core::ids::ArtifactHash;
use std::path::{Path, PathBuf};

/// A package taken out of a bundle and verified against the name it had there.
pub struct BundledPackage {
    pub hash: ArtifactHash,
    pub path: PathBuf,
}

pub struct BundleContents {
    pub recipe_json: String,
    pub packages: Vec<BundledPackage>,
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
    ) -> AppResult<PathBuf>;

    /// Reads the recipe and the names of the packages without extracting them.
    fn peek_bundle(&self, path: &Path) -> AppResult<(String, Vec<ArtifactHash>)>;

    /// Extracts the packages into `extract_dir`, verifying each digest.
    fn read_bundle(&self, path: &Path, extract_dir: &Path) -> AppResult<BundleContents>;
}
