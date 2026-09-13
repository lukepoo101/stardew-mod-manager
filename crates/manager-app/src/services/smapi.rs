use crate::api::dto::SmapiStatusDto;
use crate::error::{AppError, AppResult};
use crate::ports::repositories::{GameInstallationRepository, SmapiRepository};
use crate::ports::runtime::{DownloadPort, SmapiInspectorPort, SmapiInstallerPort};
use manager_core::ids::GameInstallationId;
use manager_core::smapi::{
    default_release_policy, get_pinned_smapi_release, ManagedSmapiInstallation, SmapiReleaseInfo,
    SmapiReleasePolicy,
};
use std::path::PathBuf;
use std::sync::Arc;

pub struct SmapiService {
    smapi_repo: Arc<dyn SmapiRepository>,
    game_repo: Arc<dyn GameInstallationRepository>,
    inspector: Arc<dyn SmapiInspectorPort>,
    installer: Arc<dyn SmapiInstallerPort>,
    downloader: Arc<dyn DownloadPort>,
    cache_dir: PathBuf,
    policy: SmapiReleasePolicy,
}

impl SmapiService {
    pub fn new(
        smapi_repo: Arc<dyn SmapiRepository>,
        game_repo: Arc<dyn GameInstallationRepository>,
        inspector: Arc<dyn SmapiInspectorPort>,
        installer: Arc<dyn SmapiInstallerPort>,
        downloader: Arc<dyn DownloadPort>,
        cache_dir: PathBuf,
    ) -> Self {
        Self {
            smapi_repo,
            game_repo,
            inspector,
            installer,
            downloader,
            cache_dir,
            policy: default_release_policy(),
        }
    }

    pub fn get_smapi_status(&self, game_id: &GameInstallationId) -> AppResult<SmapiStatusDto> {
        let game = self
            .game_repo
            .get_game(game_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game installation not found"))?;

        let observation = self.inspector.observe_smapi(&game.canonical_root)?;
        let tested = self.policy.tested_version.clone();
        let is_installed = observation.is_present;
        let is_compatible = observation
            .observed_version
            .as_ref()
            .map(|v| v.starts_with(&tested))
            .unwrap_or(is_installed);

        Ok(SmapiStatusDto {
            is_installed,
            observed_version: observation.observed_version,
            tested_version: tested,
            is_compatible,
        })
    }

    pub fn prepare_smapi(&self) -> SmapiReleaseInfo {
        get_pinned_smapi_release()
    }

    pub async fn install_smapi(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<ManagedSmapiInstallation> {
        let game = self
            .game_repo
            .get_game(game_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game installation not found"))?;

        let platform_policy = self
            .policy
            .platforms
            .get("linux")
            .ok_or_else(|| AppError::internal("Platform policy missing for linux", ""))?;

        let installer_zip = self.cache_dir.join(format!(
            "SMAPI-{}-installer.zip",
            self.policy.tested_version
        ));

        // Download installer using reqwest downloader port if not cached
        if !installer_zip.exists() {
            self.downloader
                .download_file(
                    &platform_policy.url,
                    Some(&platform_policy.sha256),
                    &installer_zip,
                )
                .await?;
        }

        // Run installer port
        let record = self
            .installer
            .install_smapi(game_id, &game.canonical_root, &installer_zip)?;

        self.smapi_repo.save_smapi_installation(&record)?;

        Ok(record)
    }
}
