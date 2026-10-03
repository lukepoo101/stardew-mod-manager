use crate::api::dto::{SetupAccessCheckDto, SetupPreviewDto, SmapiStatusDto};
use crate::error::{AppError, AppResult};
use crate::ports::launcher::GameLauncherPort;
use crate::ports::repositories::OperationRepository;
use crate::ports::repositories::{GameInstallationRepository, SmapiRepository};
use crate::ports::runtime::{DownloadPort, SmapiInspectorPort, SmapiInstallerPort};
use crate::services::operation_lifecycle::OperationLifecycle;
use crate::services::operations::recovery_state_unknown;
use crate::services::resources::{ensure_resources_available, ResourceClaim, ResourceCoordinator};
use crate::services::smapi_catalog::{KeptInstaller, SmapiCatalog};
use crate::services::RuntimeObserver;
use chrono::Utc;
use manager_core::ids::GameInstallationId;
use manager_core::ids::OperationId;
use manager_core::operation::{
    Operation, OperationKind, OperationState, OperationStepKind, OPERATION_PLAN_SCHEMA_V2,
    SMAPI_STEP_DOWNLOAD_INSTALLER, SMAPI_STEP_INSTALL_FILES, SMAPI_STEP_PERSIST_STATE,
};
use manager_core::ports::InstanceLock;
use manager_core::smapi::catalog::{
    builtin_release, compatibility, installer_file_name, recommend, Compatibility, SmapiRelease,
};
use manager_core::smapi::{
    default_release_policy, get_pinned_smapi_release, ManagedSmapiInstallation, SmapiReleaseInfo,
    SmapiReleasePolicy, PINNED_SMAPI_VERSION,
};

/// How many verified installers are kept for reinstalling and rolling back
/// without the network.
pub const KEPT_INSTALLERS: usize = 3;
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
    catalog: Option<Arc<SmapiCatalog>>,
    observer: Option<Arc<RuntimeObserver>>,
}

impl SmapiService {
    /// Lets SMAPI be installed at any published version, chosen from the
    /// releases SMAPI publishes.
    pub fn with_catalog(mut self, catalog: Arc<SmapiCatalog>) -> Self {
        self.catalog = Some(catalog);
        self
    }

    /// Lets the game version (as detected, or set by the user) decide which
    /// SMAPI releases fit.
    pub fn with_runtime_observer(mut self, observer: Arc<RuntimeObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn catalog(&self) -> Option<&Arc<SmapiCatalog>> {
        self.catalog.as_ref()
    }

    pub fn game_version(&self, game_id: &GameInstallationId) -> Option<String> {
        self.observer
            .as_ref()
            .and_then(|o| o.observe(game_id).ok())
            .and_then(|v| v.game_version)
    }

    fn releases(&self) -> Vec<SmapiRelease> {
        match &self.catalog {
            Some(catalog) => catalog.cached().releases,
            None => vec![builtin_release()],
        }
    }

    /// The release to suggest for this game: the newest stable one whose
    /// declared range includes it, or the tested one when the game version
    /// is unknown.
    pub fn recommended(&self, game_id: &GameInstallationId) -> Option<SmapiRelease> {
        let releases = self.releases();
        recommend(
            &releases,
            self.game_version(game_id).as_deref(),
            PINNED_SMAPI_VERSION,
        )
        .cloned()
    }

    /// A release by version, or the recommended one.
    pub fn resolve(
        &self,
        game_id: &GameInstallationId,
        version: Option<&str>,
    ) -> AppResult<SmapiRelease> {
        match version {
            Some(v) => {
                let found = match &self.catalog {
                    Some(catalog) => catalog.find(v),
                    None => None,
                };
                found
                    .or_else(|| (v == PINNED_SMAPI_VERSION).then(builtin_release))
                    .ok_or_else(|| {
                        AppError::validation(
                            "SMAPI_VERSION_UNKNOWN",
                            format!("SMAPI {v} is not in the list of releases. Refresh the list and choose again."),
                        )
                    })
            }
            None => Ok(self.recommended(game_id).unwrap_or_else(builtin_release)),
        }
    }
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
            catalog: None,
            observer: None,
        }
    }

    pub fn get_smapi_status(&self, game_id: &GameInstallationId) -> AppResult<SmapiStatusDto> {
        let game = self
            .game_repo
            .get_game(game_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game installation not found"))?;

        let observation = self.inspector.observe_smapi(&game.canonical_root)?;
        let game_version = self.game_version(game_id);
        let recommended = self.recommended(game_id);
        // Comparisons are against the suggested release, or the tested one
        // when nothing fits.
        let tested = recommended
            .as_ref()
            .map(|r| r.version.clone())
            .unwrap_or_else(|| self.policy.tested_version.clone());
        let is_installed = observation.is_present;
        let installed_release = observation
            .observed_version
            .as_deref()
            .and_then(|v| self.releases().into_iter().find(|r| r.version == v));
        let installed_compatibility = if !is_installed {
            "absent".to_string()
        } else {
            installed_release
                .as_ref()
                .map(|r| compatibility(r, game_version.as_deref()).key().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        };
        // Compatible when SMAPI says so, or, when that cannot be told, when it
        // is the suggested release.
        let is_compatible = match installed_compatibility.as_str() {
            "compatible" => true,
            "unknown" => observation
                .observed_version
                .as_ref()
                .map(|v| v == &tested)
                .unwrap_or(is_installed),
            _ => false,
        };

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
        let view = self.catalog.as_ref().map(|c| c.cached());
        Ok(SmapiStatusDto {
            is_installed,
            observed_version: observation.observed_version,
            update_available: comparison == "older" && recommended.is_some(),
            recommended_version: recommended.map(|r| r.version),
            tested_version: tested,
            is_compatible,
            state: state.to_string(),
            comparison: comparison.to_string(),
            evidence: observation.evidence,
            game_version,
            installed_compatibility,
            managed: self.smapi_repo.get_smapi_installation(game_id)?.is_some(),
            kept_versions: self
                .catalog
                .as_ref()
                .map(|c| c.kept_installers().into_iter().map(|k| k.version).collect())
                .unwrap_or_default(),
            catalog_source: view
                .as_ref()
                .map(|v| v.source.clone())
                .unwrap_or_else(|| "builtin".to_string()),
            catalog_checked_at: view.and_then(|v| v.checked_at),
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
        version: Option<&str>,
    ) -> AppResult<SetupPreviewDto> {
        let game = self
            .game_repo
            .get_game(game_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game installation not found"))?;
        let release = self.resolve(game_id, version)?;
        let game_version = self.game_version(game_id);
        let fit = compatibility(&release, game_version.as_deref());
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
        // A space shortfall needs room, not different permissions.
        let remedy_for = |problem: &String, game: bool| {
            if problem.contains("free space") {
                Some("Free some space on that drive, then check again.".to_string())
            } else {
                remedy(game)
            }
        };
        let mut checks = vec![{
            let result = probe(&game.canonical_root);
            SetupAccessCheckDto {
                label: "Game folder".to_string(),
                path: game_path.clone(),
                needs: "read and write, and room for SMAPI's files, to install SMAPI".to_string(),
                ok: result.is_ok(),
                remedy: result
                    .as_ref()
                    .err()
                    .and_then(|problem| remedy_for(problem, true)),
                problem: result.err(),
            }
        }];
        for (label, path) in manager_locations {
            let result = probe(path);
            checks.push(SetupAccessCheckDto {
                label: label.clone(),
                path: path.to_string_lossy().to_string(),
                needs: "read and write, and room for the download, for the manager's own files"
                    .to_string(),
                ok: result.is_ok(),
                remedy: result
                    .as_ref()
                    .err()
                    .and_then(|problem| remedy_for(problem, false)),
                problem: result.err(),
            });
        }

        let mut notices = Vec::new();
        match &fit {
            Compatibility::GameTooOld { minimum } => notices.push(format!(
                "SMAPI {} needs Stardew Valley {minimum} or newer; this game is {}.",
                release.version,
                game_version.as_deref().unwrap_or("an unknown version")
            )),
            Compatibility::GameTooNew { maximum } => notices.push(format!(
                "SMAPI {} supports Stardew Valley up to {maximum}; this game is {}.",
                release.version,
                game_version.as_deref().unwrap_or("an unknown version")
            )),
            Compatibility::Unknown => notices.push(format!(
                "Whether SMAPI {} supports this game could not be told{}.",
                release.version,
                if game_version.is_none() {
                    " (the game version could not be read)"
                } else {
                    ""
                }
            )),
            Compatibility::Compatible => {}
        }
        if release.sha256.is_none() {
            notices.push(format!(
                "SMAPI {} was published without a checksum, so the download cannot be verified against one.",
                release.version
            ));
        }
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
            smapi_source: release.installer_url.clone(),
            smapi_sha256: release.sha256.clone().unwrap_or_default(),
            supported_game_version: match (&release.min_game, &release.max_game) {
                (Some(min), Some(max)) if min == max => min.clone(),
                (Some(min), Some(max)) => format!("{min} to {max}"),
                (Some(min), None) => format!("{min}+"),
                _ => "unknown".to_string(),
            },
            checksum_published: release.sha256.is_some(),
            compatibility: fit.key().to_string(),
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
                .chain(std::iter::once(match &release.sha256 {
                    Some(sha) => format!(
                        "The SMAPI download, checked against SHA-256 {sha}, in {}",
                        self.cache_dir.to_string_lossy()
                    ),
                    None => format!(
                        "The SMAPI download (no published checksum to check against) in {}",
                        self.cache_dir.to_string_lossy()
                    ),
                }))
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
        allow_unverified: bool,
    ) -> AppResult<ManagedSmapiInstallation> {
        self.install_release(game_id, Some(previewed_version), allow_unverified)
            .await
    }

    pub fn prepare_smapi(&self) -> SmapiReleaseInfo {
        get_pinned_smapi_release()
    }

    /// Every known release with how it fits this game, the suggested one and
    /// the installed one. Reads the list online first when it is a day old,
    /// or when `refresh`.
    pub async fn catalog_view(
        &self,
        game_id: &GameInstallationId,
        refresh: bool,
    ) -> AppResult<crate::api::dto::SmapiCatalogDto> {
        let view = match &self.catalog {
            Some(catalog) => catalog.refresh(refresh).await,
            None => crate::services::smapi_catalog::CatalogView {
                releases: vec![builtin_release()],
                source: "builtin".to_string(),
                checked_at: None,
                error: None,
            },
        };
        let game_version = self.game_version(game_id);
        let recommended = recommend(
            &view.releases,
            game_version.as_deref(),
            PINNED_SMAPI_VERSION,
        )
        .map(|r| r.version.clone());
        let installed = match self.game_repo.get_game(game_id)? {
            Some(game) => {
                self.inspector
                    .observe_smapi(&game.canonical_root)?
                    .observed_version
            }
            None => None,
        };
        let kept: Vec<String> = self
            .catalog
            .as_ref()
            .map(|c| c.kept_installers().into_iter().map(|k| k.version).collect())
            .unwrap_or_default();
        Ok(crate::api::dto::SmapiCatalogDto {
            releases: view
                .releases
                .iter()
                .map(|r| crate::api::dto::SmapiReleaseDto {
                    version: r.version.clone(),
                    published_at: r.published_at.clone(),
                    prerelease: r.prerelease,
                    checksum: if r.sha256.is_some() {
                        "published".to_string()
                    } else if kept.contains(&r.version) {
                        "recorded".to_string()
                    } else {
                        "none".to_string()
                    },
                    min_game: r.min_game.clone(),
                    max_game: r.max_game.clone(),
                    compatibility: compatibility(r, game_version.as_deref()).key().to_string(),
                    notes_url: r.notes_url.clone(),
                    installer_kept: kept.contains(&r.version),
                    is_installed: installed.as_deref() == Some(r.version.as_str()),
                    is_recommended: recommended.as_deref() == Some(r.version.as_str()),
                })
                .collect(),
            source: view.source,
            checked_at: view.checked_at,
            error: view.error,
            game_version,
            recommended,
            installed,
        })
    }

    /// Installs the recommended SMAPI release.
    pub async fn install_smapi(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<ManagedSmapiInstallation> {
        self.install_release(game_id, None, false).await
    }

    /// Installs one SMAPI release (the recommended one when `version` is
    /// `None`) with its own installer. A release published without a
    /// checksum is installed only with `allow_unverified`.
    pub async fn install_release(
        &self,
        game_id: &GameInstallationId,
        version: Option<&str>,
        allow_unverified: bool,
    ) -> AppResult<ManagedSmapiInstallation> {
        let release = self.resolve(game_id, version)?;
        if release.sha256.is_none() && !allow_unverified {
            return Err(AppError::validation(
                "SMAPI_UNVERIFIED",
                format!(
                    "SMAPI {} was published without a checksum, so its download cannot be verified. Confirm to install it anyway.",
                    release.version
                ),
            ));
        }
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
                "release_policy_id": "catalog",
                "version": release.version.clone(),
                "url": release.installer_url.clone(),
                "sha256": release.sha256.clone(),
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

        let installer_zip = self.cache_dir.join(installer_file_name(&release.version));

        // The download is safe to retry, so it is its own persisted boundary.
        self.lifecycle.start_step(
            &operation_id,
            SMAPI_STEP_DOWNLOAD_INSTALLER,
            OperationStepKind::DownloadSmapiInstaller,
            serde_json::json!({
                "url": release.installer_url,
                "sha256": release.sha256,
                "version": release.version,
            }),
        )?;
        if let Err(error) = self
            .downloader
            .ensure_downloaded(
                &release.installer_url,
                release.sha256.as_deref(),
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
            serde_json::json!({ "version": release.version }),
        )?;

        // The installer mutates the game directory, so the step is persisted as
        // running before it is invoked and completed only afterwards.
        self.lifecycle.start_step(
            &operation_id,
            SMAPI_STEP_INSTALL_FILES,
            OperationStepKind::InstallSmapiFiles,
            serde_json::json!({
                "game_installation_id": game_id.to_string(),
                "version": release.version,
            }),
        )?;
        let record = match self.installer.install_smapi(
            game_id,
            &game.canonical_root,
            &installer_zip,
            &release.version,
            release.sha256.as_deref(),
        ) {
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

        // Keep this verified installer for reinstalling and rolling back
        // without the network; drop the oldest beyond the limit.
        if let Some(catalog) = &self.catalog {
            let sha = release
                .sha256
                .clone()
                .or_else(|| self.installer.archive_sha256(&installer_zip).ok());
            if let Some(sha) = sha {
                let dropped = catalog.remember_installer(
                    KeptInstaller {
                        version: release.version.clone(),
                        url: release.installer_url.clone(),
                        sha256: sha,
                        installed_at: Utc::now().to_rfc3339(),
                    },
                    KEPT_INSTALLERS,
                );
                for version in dropped {
                    self.installer.remove_cached_installer(
                        &self.cache_dir.join(installer_file_name(&version)),
                    );
                }
            }
        }

        Ok(record)
    }

    /// Removes SMAPI from the game folder with the upstream installer's own
    /// uninstall mode, then checks the folder. Mods, profiles and the
    /// manager's stored packages are not touched: profiles keep their own
    /// Mods folders outside the game.
    pub async fn uninstall_smapi(&self, game_id: &GameInstallationId) -> AppResult<()> {
        let game = self
            .game_repo
            .get_game(game_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game installation not found"))?;
        if game.management_mode == manager_core::game::ManagementMode::ExternalUnmanaged {
            return Err(AppError::validation(
                "GAME_NOT_MANAGED",
                "This installation was added without letting the manager change it, so its SMAPI is left alone.",
            ));
        }
        let observation = self.inspector.observe_smapi(&game.canonical_root)?;
        if !observation.is_present {
            return Err(AppError::validation(
                "SMAPI_NOT_PRESENT",
                "There is no SMAPI in this game folder to remove.",
            ));
        }

        let _mutation_guard = self
            .instance_lock
            .acquire_guard()
            .map_err(AppError::instance_locked)?;
        let claims = [ResourceClaim::write(
            manager_core::operation::ResourceKind::GameInstallation,
            game_id.to_string(),
        )];
        ensure_resources_available(&*self.operation_repo, &claims, None)?;
        let _resource_lease = self.resources.try_acquire(&claims)?;
        if self.launcher.is_game_running(None) {
            return Err(AppError::game_running(
                "Stop Stardew Valley before removing SMAPI",
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
                "action": "uninstall",
                "release_policy_id": self.policy.tag.clone(),
                "tested_version": self.policy.tested_version.clone(),
                "observed_version": observation.observed_version,
            })
            .to_string(),
            progress_current: Some(0),
            progress_total: Some(2),
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        };
        self.operation_repo.create_operation(&operation)?;
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

        // The uninstaller is the installed version's own installer when it
        // is known, else the recommended one; both remove SMAPI.
        let release = observation
            .observed_version
            .as_deref()
            .and_then(|v| self.resolve(game_id, Some(v)).ok())
            .filter(|r| r.sha256.is_some())
            .unwrap_or_else(|| {
                self.resolve(game_id, None)
                    .unwrap_or_else(|_| builtin_release())
            });
        let installer_zip = self.cache_dir.join(installer_file_name(&release.version));

        self.lifecycle.start_step(
            &operation_id,
            SMAPI_STEP_DOWNLOAD_INSTALLER,
            OperationStepKind::DownloadSmapiInstaller,
            serde_json::json!({
                "url": release.installer_url,
                "sha256": release.sha256,
                "action": "uninstall",
            }),
        )?;
        if let Err(error) = self
            .downloader
            .ensure_downloaded(
                &release.installer_url,
                release.sha256.as_deref(),
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
            serde_json::json!({ "action": "uninstall" }),
        )?;

        self.lifecycle.start_step(
            &operation_id,
            SMAPI_STEP_INSTALL_FILES,
            OperationStepKind::RemoveSmapiFiles,
            serde_json::json!({ "game_installation_id": game_id.to_string() }),
        )?;
        let removed = self
            .installer
            .uninstall_smapi(
                &game.canonical_root,
                &installer_zip,
                release.sha256.as_deref(),
            )
            .and_then(|()| {
                // The installer saying it worked is not enough: the folder is
                // checked.
                let after = self.inspector.observe_smapi(&game.canonical_root)?;
                if after.is_present {
                    Err(AppError::system(
                        "SMAPI_STILL_PRESENT",
                        format!(
                            "The uninstaller finished but SMAPI files remain: {}",
                            after.evidence.join("; ")
                        ),
                    ))
                } else {
                    Ok(())
                }
            });
        if let Err(error) = removed {
            self.lifecycle.fail_step(
                &operation_id,
                SMAPI_STEP_INSTALL_FILES,
                Some(error.to_string()),
            )?;
            self.lifecycle.transition(
                &operation_id,
                OperationState::Failed,
                Some(error.code.as_str()),
                Some(error.to_string()),
            )?;
            return Err(error);
        }
        self.lifecycle.complete_step(
            &operation_id,
            SMAPI_STEP_INSTALL_FILES,
            OperationStepKind::RemoveSmapiFiles,
            serde_json::json!({ "removed": true }),
        )?;
        if let Err(error) = self.smapi_repo.delete_smapi_installation(game_id) {
            return Err(self.enter_recovery(
                &operation,
                "SMAPI_STATE_PERSIST_FAILED",
                "SMAPI was removed but the managed record could not be cleared",
                &error,
            ));
        }
        self.lifecycle
            .transition(&operation_id, OperationState::Succeeded, None, None)
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
