use crate::error::AppResult;
use manager_core::ids::ArtifactHash;
use manager_core::package::PackageArtifact;
use std::path::{Path, PathBuf};

pub trait ArtifactStorePort: Send + Sync {
    fn store_artifact(&self, source_file: &Path) -> AppResult<PackageArtifact>;
    fn get_artifact_path(&self, hash: &ArtifactHash) -> AppResult<PathBuf>;
    fn has_artifact(&self, hash: &ArtifactHash) -> bool;
    fn delete_unreferenced_artifact(&self, hash: &ArtifactHash) -> AppResult<bool>;
}
