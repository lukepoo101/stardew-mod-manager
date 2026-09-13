use crate::archive::SafeZipExtractor;
use chrono::Utc;
use manager_core::domain::Package;
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
