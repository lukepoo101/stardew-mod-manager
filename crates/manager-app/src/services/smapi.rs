use crate::api::dto::SmapiStatusDto;
use crate::error::{AppError, AppResult};
use crate::ports::launcher::GameLauncherPort;
use crate::ports::repositories::OperationRepository;
use crate::ports::repositories::{GameInstallationRepository, SmapiRepository};
use crate::ports::runtime::{DownloadPort, SmapiInspectorPort, SmapiInstallerPort};
use crate::services::operation_lifecycle::OperationLifecycle;
use crate::services::operations::recovery_state_unknown;
use chrono::Utc;
use manager_core::ids::GameInstallationId;
use manager_core::ids::OperationId;
use manager_core::operation::{
    Operation, OperationKind, OperationState, OperationStepKind, OPERATION_PLAN_SCHEMA_V2,
    SMAPI_STEP_DOWNLOAD_INSTALLER, SMAPI_STEP_INSTALL_FILES, SMAPI_STEP_PERSIST_STATE,
};
use manager_core::ports::InstanceLock;
use manager_core::smapi::{
    default_release_policy, get_pinned_smapi_release, ManagedSmapiInstallation, SmapiReleaseInfo,
    SmapiReleasePolicy,
};
use std::path::PathBuf;
use std::sync::Arc;

pub struct SmapiService {
    lifecycle: OperationLifecycle,
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
            lifecycle: OperationLifecycle::new(operation_repo.clone()),
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
            plan_schema_version: OPERATION_PLAN_SCHEMA_V2,
            plan_json: serde_json::json!({
                "release_policy_id": self.policy.tag.clone(),
                "tested_version": self.policy.tested_version.clone(),
            })
            .to_string(),
            progress_current: Some(0),
            progress_total: Some(3),
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        };
        self.operation_repo.create_operation(&operation)?;
        self.lifecycle
            .transition(&operation_id, OperationState::Running, None, None)?;
        self.lifecycle
            .transition(&operation_id, OperationState::Committing, None, None)?;

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

        // The download is safe to retry, so it is its own persisted boundary.
        self.lifecycle.start_step(
            &operation_id,
            SMAPI_STEP_DOWNLOAD_INSTALLER,
            OperationStepKind::DownloadSmapiInstaller,
            serde_json::json!({
                "url": platform_policy.url,
                "sha256": platform_policy.sha256,
                "tested_version": self.policy.tested_version,
            }),
        )?;
        if let Err(error) = self
            .downloader
            .ensure_downloaded(
                &platform_policy.url,
                Some(&platform_policy.sha256),
                &installer_zip,
            )
            .await
        {
            self.lifecycle.fail_step(
                &operation_id,
                SMAPI_STEP_DOWNLOAD_INSTALLER,
                Some(error.to_string()),
            )?;
            self.lifecycle.transition(
                &operation_id,
                OperationState::Failed,
                Some("SMAPI_DOWNLOAD_FAILED"),
                Some(error.to_string()),
            )?;
            return Err(error);
        }
        self.lifecycle.complete_step(
            &operation_id,
            SMAPI_STEP_DOWNLOAD_INSTALLER,
            OperationStepKind::DownloadSmapiInstaller,
            serde_json::json!({ "tested_version": self.policy.tested_version }),
        )?;

        // The installer mutates the game directory, so the step is persisted as
        // running before it is invoked and completed only afterwards.
        self.lifecycle.start_step(
            &operation_id,
            SMAPI_STEP_INSTALL_FILES,
            OperationStepKind::InstallSmapiFiles,
            serde_json::json!({
                "game_installation_id": game_id.to_string(),
                "tested_version": self.policy.tested_version,
            }),
        )?;
        let record =
            match self
                .installer
                .install_smapi(game_id, &game.canonical_root, &installer_zip)
            {
                Ok(record) => record,
                Err(error) => {
                    self.lifecycle.fail_step(
                        &operation_id,
                        SMAPI_STEP_INSTALL_FILES,
                        Some(error.to_string()),
                    )?;
                    self.lifecycle.transition(
                        &operation_id,
                        OperationState::Failed,
                        Some("SMAPI_INSTALL_FAILED"),
                        Some(error.to_string()),
                    )?;
                    return Err(error);
                }
            };
        self.lifecycle.complete_step(
            &operation_id,
            SMAPI_STEP_INSTALL_FILES,
            OperationStepKind::InstallSmapiFiles,
            serde_json::json!({ "release_version": record.release_version }),
        )?;

        // Filesystem installation exists from here on; if the managed state
        // cannot be written, the two halves have to be reconciled later.
        self.lifecycle.start_step(
            &operation_id,
            SMAPI_STEP_PERSIST_STATE,
            OperationStepKind::PersistSmapiState,
            serde_json::json!({ "release_version": record.release_version }),
        )?;
        if let Err(error) = self.smapi_repo.save_smapi_installation(&record) {
            return Err(self.enter_recovery(
                &operation,
                "SMAPI_STATE_PERSIST_FAILED",
                "SMAPI files are installed but the managed state could not be recorded",
                &error,
            ));
        }
        self.lifecycle.complete_step(
            &operation_id,
            SMAPI_STEP_PERSIST_STATE,
            OperationStepKind::PersistSmapiState,
            serde_json::json!({ "release_version": record.release_version }),
        )?;
        self.lifecycle
            .transition(&operation_id, OperationState::Succeeded, None, None)?;

        Ok(record)
    }

    /// Records that SMAPI setup needs manual reconciliation.
    ///
    /// Persisting `RecoveryRequired` is itself authoritative: if that write
    /// fails, the returned error says so rather than pretending the state was
    /// durably recorded.
    fn enter_recovery(
        &self,
        operation: &Operation,
        code: &str,
        summary: &str,
        cause: &AppError,
    ) -> AppError {
        let evidence = serde_json::json!({
            "code": code,
            "message": summary,
            "cause": cause.to_string(),
        })
        .to_string();

        match self.lifecycle.transition(
            &operation.id,
            OperationState::RecoveryRequired,
            Some(code),
            Some(evidence),
        ) {
            // The diagnosis survives: only the recovery semantics are promoted.
            Ok(()) => cause.clone().into_recovery_required(operation.id),
            Err(persist_error) => recovery_state_unknown(operation.id, cause, &persist_error),
        }
    }
}
