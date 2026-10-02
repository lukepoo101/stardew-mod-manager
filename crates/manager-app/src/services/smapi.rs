use crate::api::dto::{SetupAccessCheckDto, SetupPreviewDto, SmapiStatusDto};
use crate::error::{AppError, AppResult};
use crate::ports::launcher::GameLauncherPort;
use crate::ports::repositories::OperationRepository;
use crate::ports::repositories::{GameInstallationRepository, SmapiRepository};
use crate::ports::runtime::{DownloadPort, SmapiInspectorPort, SmapiInstallerPort};
use crate::services::operation_lifecycle::OperationLifecycle;
use crate::services::operations::recovery_state_unknown;
use crate::services::resources::{ensure_resources_available, ResourceClaim, ResourceCoordinator};
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
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How an installed SMAPI version relates to the tested one.
pub fn compare_to_tested(installed: bool, observed: Option<&str>, tested: &str) -> &'static str {
    use manager_core::version::SmapiVersion;
    if !installed {
        return "absent";
    }
    match observed.map(|v| (SmapiVersion::parse(v), SmapiVersion::parse(tested))) {
        Some((Ok(have), Ok(want))) if have == want => "same",
        Some((Ok(have), Ok(want))) if have > want => "newer",
        Some((Ok(_), Ok(_))) => "older",
        _ => "unknown",
    }
}

pub struct SmapiService {
    lifecycle: OperationLifecycle,
    resources: Arc<ResourceCoordinator>,
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
        resources: Arc<ResourceCoordinator>,
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
            resources,
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

        let state = if !is_installed {
            "absent"
        } else if observation.artifacts_complete {
            "installed"
        } else {
            "partial"
        };
        let comparison = compare_to_tested(
            is_installed,
            observation.observed_version.as_deref(),
            &tested,
        );
        Ok(SmapiStatusDto {
            is_installed,
            observed_version: observation.observed_version,
            tested_version: tested,
            is_compatible,
            state: state.to_string(),
            comparison: comparison.to_string(),
            evidence: observation.evidence,
        })
    }

    /// What installing SMAPI into `game_id` will do and whether every
    /// location it needs can be used. `manager_locations` are the folders the
    /// manager owns (label, path); `probe` reports why a folder cannot be
    /// read and written. Nothing is changed.
    pub fn preview_setup(
        &self,
        game_id: &GameInstallationId,
        manager_locations: &[(String, PathBuf)],
        probe: &dyn Fn(&Path) -> Result<(), String>,
    ) -> AppResult<SetupPreviewDto> {
        let game = self
            .game_repo
            .get_game(game_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game installation not found"))?;
        let release = get_pinned_smapi_release();
        let observation = self.inspector.observe_smapi(&game.canonical_root)?;
        let game_path = game.canonical_root.to_string_lossy().to_string();

        let remedy = |game: bool| {
            Some(if game {
                "Make the game folder writable for your user, or move the game to a library you own. The manager never asks for administrator rights.".to_string()
            } else {
                "Check that this folder belongs to your user and the drive is not read-only."
                    .to_string()
            })
        };
        let mut checks = vec![{
            let result = probe(&game.canonical_root);
            SetupAccessCheckDto {
                label: "Game folder".to_string(),
                path: game_path.clone(),
                needs: "read and write, to install SMAPI".to_string(),
                ok: result.is_ok(),
                remedy: result.as_ref().err().and_then(|_| remedy(true)),
                problem: result.err(),
            }
        }];
        for (label, path) in manager_locations {
            let result = probe(path);
            checks.push(SetupAccessCheckDto {
                label: label.clone(),
                path: path.to_string_lossy().to_string(),
                needs: "read and write, for the manager's own files".to_string(),
                ok: result.is_ok(),
                remedy: result.as_ref().err().and_then(|_| remedy(false)),
                problem: result.err(),
            });
        }

        let mut notices = Vec::new();
        if observation.is_present {
            notices.push(format!(
                "SMAPI {} is already in the game folder. The installer will update or repair it in place.",
                observation
                    .observed_version
                    .as_deref()
                    .unwrap_or("(version unknown)")
            ));
        }
        let can_proceed = checks.iter().all(|c| c.ok)
            && game.management_mode == manager_core::game::ManagementMode::Managed;
        if game.management_mode != manager_core::game::ManagementMode::Managed {
            notices.push(
                "This installation was added without being managed, so SMAPI is not installed here."
                    .to_string(),
            );
        }

        Ok(SetupPreviewDto {
            game_path: game_path.clone(),
            smapi_version: release.version.clone(),
            smapi_source: release.asset_url.clone(),
            smapi_sha256: release.sha256.clone(),
            supported_game_version: release.supported_game_version.clone(),
            installed_smapi: observation
                .is_present
                .then(|| observation.observed_version.clone().unwrap_or_default()),
            modifies: vec![
                format!(
                    "Runs the official SMAPI {} installer on {game_path}",
                    release.version
                ),
                "Adds the SMAPI launcher and its smapi-internal folder there".to_string(),
                "Adds the mods SMAPI ships with (Console Commands, Save Backup) to the game's Mods folder".to_string(),
            ],
            creates: manager_locations
                .iter()
                .map(|(label, path)| format!("{label}: {}", path.to_string_lossy()))
                .chain(std::iter::once(format!(
                    "The SMAPI download, checked against SHA-256 {}, in {}",
                    release.sha256,
                    self.cache_dir.to_string_lossy()
                )))
                .collect(),
            reads: vec![
                "The game's files, to confirm the version and that SMAPI installed".to_string(),
                "Your saves are not read or changed by setup".to_string(),
            ],
            notices,
            checks,
            can_proceed,
        })
    }

    /// Installs SMAPI only if the release is still the one that was
    /// previewed; a different one has to be previewed again first.
    pub async fn install_smapi_as_previewed(
        &self,
        game_id: &GameInstallationId,
        previewed_version: &str,
    ) -> AppResult<ManagedSmapiInstallation> {
        let release = get_pinned_smapi_release();
        if release.version != previewed_version {
            return Err(AppError::validation(
                "SETUP_PLAN_CHANGED",
                format!(
                    "Setup would now install SMAPI {}, not the {} you reviewed. Review the changes again.",
                    release.version, previewed_version
                ),
            ));
        }
        self.install_smapi(game_id).await
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
        if game.management_mode == manager_core::game::ManagementMode::ExternalUnmanaged {
            return Err(AppError::validation(
                "GAME_NOT_MANAGED",
                "This installation was added without letting the manager change it, so SMAPI is not installed here. Its SMAPI and Mods folder stay as they are.",
            ));
        }

        let _mutation_guard = self
            .instance_lock
            .acquire_guard()
            .map_err(AppError::instance_locked)?;
        // SMAPI setup mutates the game directory, so it excludes every other
        // user of that installation.
        let claims = [ResourceClaim::write(
            manager_core::operation::ResourceKind::GameInstallation,
            game_id.to_string(),
        )];
        // Durable ownership first: an unresolved SMAPI setup from an earlier
        // run still owns this installation, even though no in-process lease
        // survived the restart.
        ensure_resources_available(&*self.operation_repo, &claims, None)?;
        // Then in-process ownership: what is executing right now.
        let _resource_lease = self.resources.try_acquire(&claims)?;
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
        // The durable declaration is what makes this ownership survive a
        // restart, so it is recorded before the live mutation lifecycle begins.
        self.operation_repo.save_operation_resource(
            &manager_core::operation::OperationResource {
                operation_id,
                resource_kind: manager_core::operation::ResourceKind::GameInstallation,
                resource_id: game_id.to_string(),
                access_mode: manager_core::operation::AccessMode::Write,
            },
        )?;
        self.lifecycle
            .transition(&operation_id, OperationState::Running, None, None)?;
        self.lifecycle
            .transition(&operation_id, OperationState::Committing, None, None)?;

        let platform_key = game.operating_system.as_key();
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

#[cfg(test)]
mod comparison_tests {
    use super::compare_to_tested;

    #[test]
    fn installed_versions_are_compared_with_the_tested_one() {
        assert_eq!(compare_to_tested(false, None, "4.1.10"), "absent");
        assert_eq!(compare_to_tested(true, None, "4.1.10"), "unknown");
        assert_eq!(compare_to_tested(true, Some("4.1.10"), "4.1.10"), "same");
        assert_eq!(compare_to_tested(true, Some("4.2.0"), "4.1.10"), "newer");
        assert_eq!(compare_to_tested(true, Some("4.1.9"), "4.1.10"), "older");
        assert_eq!(compare_to_tested(true, Some("weird"), "4.1.10"), "unknown");
    }
}
