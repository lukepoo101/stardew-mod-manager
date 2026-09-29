use crate::archive::SafeZipExtractor;
use crate::platform::shared::fs;
use chrono::Utc;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::artifacts::ArtifactStorePort;
use manager_core::ids::ArtifactHash;
use manager_core::package::PackageArtifact;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct FilesystemPackageStore {
    packages_dir: PathBuf,
}

impl FilesystemPackageStore {
    pub fn new<P: AsRef<Path>>(packages_dir: P) -> Self {
        Self {
            packages_dir: packages_dir.as_ref().to_path_buf(),
        }
    }
}

impl ArtifactStorePort for FilesystemPackageStore {
    fn store_artifact(&self, source_file: &Path) -> AppResult<PackageArtifact> {
        let (hash_str, size) = SafeZipExtractor::compute_sha256(source_file)
            .map_err(|e| AppError::filesystem("Failed to compute SHA256", e))?;
        let hash = ArtifactHash::new(hash_str);

        std::fs::create_dir_all(&self.packages_dir).map_err(|e| {
            AppError::filesystem("Failed to create packages directory", e.to_string())
        })?;

        let dest = self.packages_dir.join(format!("{}.zip", hash.as_str()));
        if dest.exists() {
            let (existing_hash, _) = SafeZipExtractor::compute_sha256(&dest)
                .map_err(|e| AppError::filesystem("Failed to re-hash stored artifact", e))?;
            if existing_hash != hash.as_str() {
                fs::remove_file(&dest).map_err(|e| {
                    AppError::filesystem("Failed to replace corrupt artifact", e.to_string())
                })?;
            }
        }

        if !dest.exists() {
            let tmp =
                self.packages_dir
                    .join(format!("{}.tmp.{}", hash.as_str(), uuid::Uuid::new_v4()));
            fs::copy_file(source_file, &tmp).map_err(|e| {
                AppError::filesystem("Failed to copy package archive", e.to_string())
            })?;
            // The promotion is the last step of a content-addressed write; a
            // scanner holding the temporary file open is transient, so it is
            // retried before reporting a failure the user would have to act on.
            fs::rename_path(&tmp, &dest).map_err(|e| {
                AppError::filesystem("Failed to rename temporary package archive", e.to_string())
            })?;
        }

        Ok(PackageArtifact {
            hash: hash.clone(),
            byte_size: size,
            storage_relative_path: format!("packages/{}.zip", hash.as_str()),
            first_seen_at: Utc::now(),
        })
    }

    fn get_artifact_path(&self, hash: &ArtifactHash) -> AppResult<PathBuf> {
        let path = self.packages_dir.join(format!("{}.zip", hash.as_str()));
        if path.exists() {
            Ok(path)
        } else {
            Err(AppError::filesystem(
                "Package artifact not found",
                format!("Path does not exist: {}", path.display()),
            ))
        }
    }

    fn has_artifact(&self, hash: &ArtifactHash) -> bool {
        self.packages_dir
            .join(format!("{}.zip", hash.as_str()))
            .exists()
    }

    fn delete_unreferenced_artifact(&self, hash: &ArtifactHash) -> AppResult<bool> {
        let path = self.packages_dir.join(format!("{}.zip", hash.as_str()));
        if path.exists() {
            fs::remove_file(&path).map_err(|e| {
                AppError::filesystem("Failed to remove unreferenced artifact", e.to_string())
            })?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
