use crate::api::dto::SmapiStatusDto;
use crate::error::{AppError, AppResult};
use crate::ports::launcher::GameLauncherPort;
use crate::ports::repositories::OperationRepository;
use crate::ports::repositories::{GameInstallationRepository, SmapiRepository};
use crate::ports::runtime::{DownloadPort, SmapiInspectorPort, SmapiInstallerPort};
use chrono::Utc;
use manager_core::ids::GameInstallationId;
use manager_core::ids::OperationId;
use manager_core::operation::{Operation, OperationKind, OperationState};
use manager_core::ports::InstanceLock;
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
    operation_repo: Arc<dyn OperationRepository>,
    launcher: Arc<dyn GameLauncherPort>,
    instance_lock: Arc<dyn InstanceLock>,
}

impl SmapiService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        smapi_repo: Arc<dyn SmapiRepository>,
        game_repo: Arc<dyn GameInstallationRepository>,
        inspector: Arc<dyn SmapiInspectorPort>,
        installer: Arc<dyn SmapiInstallerPort>,
        downloader: Arc<dyn DownloadPort>,
        cache_dir: PathBuf,
        operation_repo: Arc<dyn OperationRepository>,
        launcher: Arc<dyn GameLauncherPort>,
        instance_lock: Arc<dyn InstanceLock>,
    ) -> Self {
        Self {
            smapi_repo,
            game_repo,
            inspector,
            installer,
            downloader,
            cache_dir,
            policy: default_release_policy(),
            operation_repo,
            launcher,
            instance_lock,
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

        let _mutation_guard = self
            .instance_lock
            .acquire_guard()
            .map_err(AppError::instance_locked)?;
        if self.launcher.is_game_running(None) {
            return Err(AppError::game_running(
                "Stop Stardew Valley before installing SMAPI",
            ));
        }
        let operation_id = OperationId::new();
        let operation = Operation {
            id: operation_id,
            kind: OperationKind::SmapiSetup,
            state: OperationState::Prepared,
            game_installation_id: Some(*game_id),
            profile_id: None,
            expected_profile_revision: None,
            plan_schema_version: 1,
            plan_json: serde_json::json!({
                "release_policy_id": self.policy.tag.clone(),
                "tested_version": self.policy.tested_version.clone(),
            })
            .to_string(),
            progress_current: Some(0),
            progress_total: Some(1),
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        };
        self.operation_repo.save_operation(&operation)?;
        self.operation_repo.update_operation_state(
            &operation_id,
            OperationState::Running,
            None,
            None,
        )?;
        self.operation_repo.update_operation_state(
            &operation_id,
            OperationState::Committing,
            None,
            None,
        )?;

        let platform_key = match game.operating_system {
            manager_core::game::OperatingSystem::Linux => "linux",
            manager_core::game::OperatingSystem::Windows => "windows",
            manager_core::game::OperatingSystem::MacOS => "macos",
        };
        let platform_policy = self
            .policy
            .platforms
            .get(platform_key)
            .ok_or_else(|| AppError::internal("SMAPI_PLATFORM_POLICY_MISSING", platform_key))?;

        let installer_zip = self.cache_dir.join(format!(
            "SMAPI-{}-installer.zip",
            self.policy.tested_version
        ));

        self.downloader
            .ensure_downloaded(
                &platform_policy.url,
                Some(&platform_policy.sha256),
                &installer_zip,
            )
            .await?;

        // Run installer port
        let record =
            match self
                .installer
                .install_smapi(game_id, &game.canonical_root, &installer_zip)
            {
                Ok(record) => record,
                Err(error) => {
                    let _ = self.operation_repo.update_operation_state(
                        &operation_id,
                        OperationState::Failed,
                        Some("SMAPI_INSTALL_FAILED".to_string()),
                        Some(error.to_string()),
                    );
                    return Err(error);
                }
            };

        if let Err(error) = self.smapi_repo.save_smapi_installation(&record) {
            let _ = self.operation_repo.update_operation_state(
                &operation_id,
                OperationState::RecoveryRequired,
                Some("SMAPI_STATE_PERSIST_FAILED".to_string()),
                Some(error.to_string()),
            );
            return Err(error);
        }
        self.operation_repo.update_operation_state(
            &operation_id,
            OperationState::Succeeded,
            None,
            None,
        )?;

        Ok(record)
    }
}
