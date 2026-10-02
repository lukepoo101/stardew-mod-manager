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

        // Installing again from the manager's own stored copy (a reinstall,
        // version change or restore) is not a new acquisition: keep the record
        // of where the package really came from.
        let stored = self.artifact_store.get_artifact_path(&artifact.hash)?;
        let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => a == b,
        };
        if same(source_path, &stored) {
            if let Some(original) = self
                .catalog_repo
                .get_acquisitions_for_artifact(&artifact.hash)?
                .into_iter()
                .min_by_key(|a| a.acquired_at)
            {
                return Ok((artifact, original));
            }
        }

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

    /// Whether the stored archive still hashes to its recorded digest.
    pub fn verify_artifact(&self, hash: &ArtifactHash) -> AppResult<bool> {
        self.artifact_store.verify_artifact(hash)
    }

    pub fn has_artifact(&self, hash: &ArtifactHash) -> bool {
        self.artifact_store.has_artifact(hash)
    }

    pub fn get_artifact(&self, hash: &ArtifactHash) -> AppResult<Option<PackageArtifact>> {
        self.catalog_repo.get_artifact(hash)
    }

    /// Stored, intact packages that contain a mod with `unique_id`, newest
    /// version first. Each says whether it meets `minimum`; one whose
    /// version cannot be compared is marked as unknown rather than meeting it.
    pub fn stored_with_unique_id(
        &self,
        unique_id: &str,
        minimum: Option<&str>,
    ) -> AppResult<Vec<crate::api::dto::StoredCandidateDto>> {
        use manager_core::version::SmapiVersion;
        let wanted = unique_id.trim().to_lowercase();
        let mut found = Vec::new();
        for artifact in self.catalog_repo.list_artifacts()? {
            if !self.artifact_store.has_artifact(&artifact.hash) {
                continue;
            }
            for component in self
                .catalog_repo
                .list_components_for_artifact(&artifact.hash)?
            {
                if component.unique_id.as_str().to_lowercase() != wanted {
                    continue;
                }
                let meets = match minimum {
                    None => Some(true),
                    Some(min) => match (
                        SmapiVersion::parse(&component.version),
                        SmapiVersion::parse(min),
                    ) {
                        (Ok(have), Ok(need)) => Some(have >= need),
                        _ => None,
                    },
                };
                let original_filename = self
                    .catalog_repo
                    .get_acquisitions_for_artifact(&artifact.hash)?
                    .into_iter()
                    .next()
                    .map(|a| a.original_filename)
                    .unwrap_or_default();
                found.push(crate::api::dto::StoredCandidateDto {
                    artifact_hash: artifact.hash.as_str().to_string(),
                    name: component.name.clone(),
                    version: component.version.clone(),
                    original_filename,
                    meets_minimum: meets,
                });
            }
        }
        found.sort_by(|a, b| {
            match (
                SmapiVersion::parse(&b.version),
                SmapiVersion::parse(&a.version),
            ) {
                (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                _ => b.version.cmp(&a.version),
            }
        });
        Ok(found)
    }
}
