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

    /// Runs the upstream installer's own uninstall mode. Success here is not
    /// proof; the caller checks the game folder afterwards.
    fn uninstall_smapi(&self, game_path: &Path, installer_archive: &Path) -> AppResult<()>;
}

#[async_trait]
pub trait DownloadPort: Send + Sync {
    async fn ensure_downloaded(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        destination: &Path,
    ) -> AppResult<PathBuf>;

    async fn download_file(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        destination: &Path,
    ) -> AppResult<PathBuf>;
}
