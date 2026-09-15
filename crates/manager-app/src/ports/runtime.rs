use crate::error::AppResult;
use async_trait::async_trait;
use manager_core::ids::GameInstallationId;
use manager_core::smapi::{ManagedSmapiInstallation, SmapiObservation};
use std::path::{Path, PathBuf};

pub trait SmapiInspectorPort: Send + Sync {
    fn observe_smapi(&self, game_dir: &Path) -> AppResult<SmapiObservation>;
}

pub trait SmapiInstallerPort: Send + Sync {
    fn install_smapi(
        &self,
        game_id: &GameInstallationId,
        game_path: &Path,
        installer_archive: &Path,
    ) -> AppResult<ManagedSmapiInstallation>;
}

#[async_trait]
pub trait DownloadPort: Send + Sync {
    async fn download_file(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        destination: &Path,
    ) -> AppResult<PathBuf>;
}
