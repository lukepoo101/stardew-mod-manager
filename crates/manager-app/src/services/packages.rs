use crate::error::AppResult;
use crate::ports::artifacts::ArtifactStorePort;
use crate::ports::repositories::PackageCatalogRepository;
use chrono::Utc;
use manager_core::ids::{AcquisitionId, ArtifactHash};
use manager_core::package::{Acquisition, AcquisitionSource, PackageArtifact};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct PackagesService {
    catalog_repo: Arc<dyn PackageCatalogRepository>,
    artifact_store: Arc<dyn ArtifactStorePort>,
}

impl PackagesService {
    pub fn new(
        catalog_repo: Arc<dyn PackageCatalogRepository>,
        artifact_store: Arc<dyn ArtifactStorePort>,
    ) -> Self {
        Self {
            catalog_repo,
            artifact_store,
        }
    }

    pub fn retain_local_package(
        &self,
        source_path: &Path,
    ) -> AppResult<(PackageArtifact, Acquisition)> {
        let artifact = self.artifact_store.store_artifact(source_path)?;
        self.catalog_repo.save_artifact(&artifact)?;

        let filename = source_path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "mod.zip".to_string());

        let acquisition = Acquisition {
            id: AcquisitionId::new(),
            artifact_hash: artifact.hash.clone(),
            source: AcquisitionSource::LocalFile,
            original_filename: filename,
            acquired_at: Utc::now(),
            expected_hash: None,
            source_metadata: None,
        };
        self.catalog_repo.save_acquisition(&acquisition)?;

        Ok((artifact, acquisition))
    }

    pub fn get_artifact_path(&self, hash: &ArtifactHash) -> AppResult<PathBuf> {
        self.artifact_store.get_artifact_path(hash)
    }

    pub fn get_artifact(&self, hash: &ArtifactHash) -> AppResult<Option<PackageArtifact>> {
        self.catalog_repo.get_artifact(hash)
    }
}
