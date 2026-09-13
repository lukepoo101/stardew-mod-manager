use crate::archive::SafeZipExtractor;
use chrono::Utc;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::artifacts::ArtifactStorePort;
use manager_core::domain::Package;
use manager_core::ids::ArtifactHash;
use manager_core::package::PackageArtifact;
use manager_core::ports::PackageStore;
use std::path::{Path, PathBuf};

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
        if !dest.exists() {
            let tmp =
                self.packages_dir
                    .join(format!("{}.tmp.{}", hash.as_str(), uuid::Uuid::new_v4()));
            std::fs::copy(source_file, &tmp).map_err(|e| {
                AppError::filesystem("Failed to copy package archive", e.to_string())
            })?;
            std::fs::rename(&tmp, &dest).map_err(|e| {
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
            std::fs::remove_file(&path).map_err(|e| {
                AppError::filesystem("Failed to remove unreferenced artifact", e.to_string())
            })?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl PackageStore for FilesystemPackageStore {
    fn store_package(&self, source_zip: &Path) -> Result<Package, String> {
        let (hash, size) = SafeZipExtractor::compute_sha256(source_zip)?;

        std::fs::create_dir_all(&self.packages_dir)
            .map_err(|e| format!("Failed to create packages directory: {}", e))?;

        let dest = self.packages_dir.join(format!("{}.zip", hash));
        if !dest.exists() {
            std::fs::copy(source_zip, &dest)
                .map_err(|e| format!("Failed to copy package archive to store: {}", e))?;
        }

        let original_filename = source_zip
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "package.zip".to_string());

        Ok(Package {
            hash,
            original_filename,
            source_kind: "local_zip".to_string(),
            byte_size: size,
            created_at: Utc::now(),
        })
    }

    fn get_package_path(&self, hash: &str) -> Result<PathBuf, String> {
        let path = self.packages_dir.join(format!("{}.zip", hash));
        if path.exists() {
            Ok(path)
        } else {
            Err(format!("Package archive for hash '{}' not found", hash))
        }
    }
}
