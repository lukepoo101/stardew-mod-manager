use crate::api::dto::LaunchSessionDto;
use crate::error::{AppError, AppResult};
use crate::ports::deployment::DeploymentPort;
use crate::ports::launcher::{GameLauncherPort, RecordedProcessState};
use crate::ports::logging::{ExpectedMod, SessionLogPort};
use crate::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, LaunchSessionRepository, OperationRepository,
    PackageCatalogRepository, ProfileRepository, SmapiRepository,
};
use crate::ports::runtime_layout::GameRuntimePort;
use crate::services::resources::{
    conflicting_holder, ensure_resources_available, ResourceClaim, ResourceCoordinator,
};
use crate::services::runtime_observer::RuntimeObserver;
use chrono::{Duration, Utc};
use manager_core::dependency::evaluate_bundle_dependencies;
use manager_core::ids::{LaunchSessionId, ProfileId};
use manager_core::launch::{
    LaunchMode, LaunchSession, PreflightCheck, SessionState, VerificationResult,
};
use manager_core::operation::ResourceKind;
use manager_core::ports::InstanceLock;
use std::sync::Arc;

/// How long a running session may go without load evidence before verification
/// is reported as unavailable rather than silently pending.
const VERIFICATION_TIMEOUT_SECONDS: i64 = 120;

/// How many sessions' SMAPI logs are kept.
pub const SESSION_LOGS_KEPT: usize = 20;

pub struct LaunchService {
    resources: Arc<ResourceCoordinator>,
    game_repo: Arc<dyn GameInstallationRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    smapi_repo: Arc<dyn SmapiRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    session_repo: Arc<dyn LaunchSessionRepository>,
    launcher: Arc<dyn GameLauncherPort>,
    deployment: Arc<dyn DeploymentPort>,
    log_reader: Arc<dyn SessionLogPort>,
    instance_lock: Arc<dyn InstanceLock>,
    runtime: Arc<dyn GameRuntimePort>,
    observer: Option<Arc<RuntimeObserver>>,
    known_good: Option<Arc<crate::services::KnownGood>>,
    /// Sessions this run of the manager has seen running, so an exit it
    /// watched can be told from one that happened while it was closed.
    seen_running: std::sync::Mutex<std::collections::HashSet<LaunchSessionId>>,
    log_archive: Option<Arc<dyn crate::ports::logging::SessionLogArchivePort>>,
}

impl LaunchService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resources: Arc<ResourceCoordinator>,
        game_repo: Arc<dyn GameInstallationRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        smapi_repo: Arc<dyn SmapiRepository>,
        operation_repo: Arc<dyn OperationRepository>,
        session_repo: Arc<dyn LaunchSessionRepository>,
        launcher: Arc<dyn GameLauncherPort>,
        deployment: Arc<dyn DeploymentPort>,
        log_reader: Arc<dyn SessionLogPort>,
        instance_lock: Arc<dyn InstanceLock>,
        runtime: Arc<dyn GameRuntimePort>,
    ) -> Self {
        Self {
            resources,
            game_repo,
            profile_repo,
            deployment_repo,
            package_repo,
            smapi_repo,
            operation_repo,
            session_repo,
            launcher,
            deployment,
            log_reader,
            instance_lock,
            runtime,
            observer: None,
            known_good: None,
            log_archive: None,
            seen_running: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Lets the service remember which game and SMAPI versions a profile last
    /// loaded its mods with, so a later change can be reported.
    /// Records the profile's mods whenever a modded session is confirmed.
    pub fn with_known_good(mut self, known_good: Arc<crate::services::KnownGood>) -> Self {
        self.known_good = Some(known_good);
        self
    }

    /// Keeps a copy of each session's SMAPI log when the session ends.
    pub fn with_log_archive(
        mut self,
        archive: Arc<dyn crate::ports::logging::SessionLogArchivePort>,
    ) -> Self {
        self.log_archive = Some(archive);
        self
    }

    pub fn with_runtime_observer(mut self, observer: Arc<RuntimeObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// The transient claims a launch holds over the profile and its game.
    ///
    /// Launching is not a durable operation, so it declares these in memory
    /// rather than inventing an operation row - but the same claims are also
    /// checked against durable unresolved ownership.
    fn launch_claims(
        &self,
        profile_id: &ProfileId,
        game_id: &manager_core::ids::GameInstallationId,
    ) -> AppResult<Vec<ResourceClaim>> {
        Ok(vec![
            ResourceClaim::read(ResourceKind::Profile, profile_id.to_string()),
            ResourceClaim::read(ResourceKind::GameInstallation, game_id.to_string()),
        ])
    }

    /// Whether the process a persisted session recorded is still running.
    ///
    /// The identity, not the pid, is what is checked: a pid is recycled, so
    /// "pid 4242 is alive" after a restart says nothing about whether it is the
    /// game this manager started. An identity that cannot be re-established is
    /// reported as unknown, and callers treat unknown conservatively rather
    /// than pretending the session is still live.
    fn recorded_session_state(&self, session: &LaunchSession) -> RecordedProcessState {
        let state = self.check_recorded_session_state(session);
        if state != RecordedProcessState::Exited {
            self.seen_running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(session.id);
        }
        state
    }

    fn check_recorded_session_state(&self, session: &LaunchSession) -> RecordedProcessState {
        if let Some(identity) = session.process_identity.as_ref() {
            return self.launcher.identify_recorded(identity);
        }
        // Sessions recorded before identity was persisted fall back to the pid
        // check, which is the best evidence that exists for them.
        match session.pid {
            Some(pid) => {
                if self.launcher.is_game_running(Some(pid)) {
                    RecordedProcessState::Running
                } else {
                    RecordedProcessState::Exited
                }
            }
            None => RecordedProcessState::Exited,
        }
    }

    pub fn get_launch_preflight(
        &self,
        profile_id: &ProfileId,
        mode: LaunchMode,
    ) -> AppResult<PreflightCheck> {
        let mut blockers = Vec::new();
        let mut warnings = Vec::new();

        let profile = self
            .profile_repo
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;

        let game = self
            .game_repo
            .get_game(&profile.game_installation_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game installation not found"))?;

        let smapi_installation = if mode != LaunchMode::Vanilla {
            self.smapi_repo.get_smapi_installation(&game.id)?
        } else {
            None
        };

        if mode != LaunchMode::Vanilla && smapi_installation.is_none() {
            blockers.push("SMAPI is not installed for this game installation".to_string());
        }

        // The exact launch that would start: its game folder and launcher
        // have to be there.
        let mods_root = self.deployment.get_profile_mods_root(profile_id);
        match self
            .runtime
            .build_launch_spec(&game, mode, Some(mods_root.as_path()))
        {
            Ok(spec) => {
                let missing = self.runtime.missing_launch_files(&spec);
                if !missing.is_empty() {
                    blockers.push(format!(
                        "The game cannot be started because {} is missing. Check the game installation, or run setup again from Settings.",
                        missing.join(" and ")
                    ));
                }
            }
            Err(error) => blockers.push(error.to_string()),
        }

        // Both claims a launch depends on are checked durably: the profile it
        // reads and the game installation it starts. An unresolved SMAPI setup
        // owns the game installation even though it has no profile.
        let launch_claims = self.launch_claims(profile_id, &game.id)?;
        let mut held = Vec::new();
        for claim in &launch_claims {
            held.extend(
                self.operation_repo
                    .list_unresolved_resources(claim.kind, &claim.resource_id)?,
            );
        }
        if conflicting_holder(&launch_claims, held.iter()).is_some() {
            blockers.push(
                "This profile or its game installation has an unresolved or recovering operation"
                    .to_string(),
            );
        }

        // A runtime test starts SMAPI with no mods, so the profile's mods are
        // not checked for it.
        if mode == LaunchMode::Modded {
            let mut manifests = Vec::new();
            for pc in self.deployment_repo.list_profile_components(profile_id)? {
                if !pc.enabled {
                    continue;
                }
                if let Some(comp) = self
                    .package_repo
                    .get_package_component(&pc.package_component_id)?
                {
                    manifests.push(comp.manifest);
                }
            }

            let current_smapi_version = smapi_installation
                .as_ref()
                .map(|installation| installation.release_version.as_str());
            let report = evaluate_bundle_dependencies(&manifests, &[], current_smapi_version);

            if !report.smapi_compatible {
                blockers.push(format!(
                    "At least one enabled mod requires a newer SMAPI version than {}",
                    current_smapi_version.unwrap_or("the currently installed runtime")
                ));
            }
            if report.duplicate_id {
                blockers.push(
                    "Two enabled components expose the same SMAPI UniqueID in this profile"
                        .to_string(),
                );
            }
            for finding in report.findings {
                if finding.is_required && !finding.satisfied && !blockers.contains(&finding.reason)
                {
                    blockers.push(finding.reason);
                }
            }
        }

        // The recorded enabled state has to match the files SMAPI will scan, or
        // the mods that load will not be the mods the profile says are on.
        if mode == LaunchMode::Modded {
            let mut seen = std::collections::HashSet::new();
            for pc in self.deployment_repo.list_profile_components(profile_id)? {
                if !seen.insert(pc.deployment_id) {
                    continue;
                }
                let Some(deployment) = self.deployment_repo.get_deployment(&pc.deployment_id)?
                else {
                    continue;
                };
                let present = self
                    .deployment
                    .deployment_exists(profile_id, &deployment.root_relative_path)?;
                if pc.enabled && !present {
                    blockers.push(format!(
                        "{} is marked enabled but its files are not in the mods folder. Disable and enable it again to repair this.",
                        deployment.root_relative_path
                    ));
                } else if !pc.enabled && present {
                    blockers.push(format!(
                        "{} is marked disabled but its files are still in the mods folder. Enable and disable it again to repair this.",
                        deployment.root_relative_path
                    ));
                }
            }
        }

        // Check if game is already running
        if self.launcher.is_game_running(None) {
            blockers.push("Game is already running".to_string());
        }
        if let Some(latest) = self
            .session_repo
            .get_latest_launch_session(Some(profile_id))?
        {
            if (latest.state == SessionState::Starting
                || latest.state == SessionState::RunningUnverified
                || latest.state == SessionState::ModLoadConfirmed)
                && self.recorded_session_state(&latest) != RecordedProcessState::Exited
            {
                blockers.push("Game is already running".to_string());
            }
        }

        // Warnings never block a launch, but they must not let an unknown read
        // as good news either.
        if let Some(latest) = self
            .session_repo
            .get_latest_launch_session(Some(profile_id))?
        {
            if latest.state == SessionState::VerificationUnavailable {
                warnings.push(
                    "The last launch of this profile could not be verified from the SMAPI log."
                        .to_string(),
                );
            }
        }
        if mode == LaunchMode::Modded {
            warnings.push(
                "Compatibility of your mods with this game version has not been assessed."
                    .to_string(),
            );
        }

        let can_launch = blockers.is_empty();
        Ok(PreflightCheck {
            can_launch,
            blockers,
            warnings,
        })
    }

    pub fn launch_profile(
        &self,
        profile_id: &ProfileId,
        mode: LaunchMode,
    ) -> AppResult<LaunchSessionDto> {
        self.launch_profile_acknowledging(profile_id, mode, None)
    }

    /// Launches after the user reviewed the preflight warnings in
    /// `acknowledged`. Blockers are never overridable; if the warnings are
    /// not the ones reviewed (a new one appeared, or one changed), nothing
    /// starts and they have to be reviewed again. The accepted warnings are
    /// kept on the session; they are not dismissed anywhere else.
    pub fn launch_profile_acknowledging(
        &self,
        profile_id: &ProfileId,
        mode: LaunchMode,
        acknowledged: Option<&[String]>,
    ) -> AppResult<LaunchSessionDto> {
        let _launch_guard = self
            .instance_lock
            .acquire_guard()
            .map_err(AppError::instance_locked)?;

        // A launch reads the profile's deployment tree and its game
        // installation. Those claims are transient: launching is not a durable
        // operation, so it participates in coordination without inventing a fake
        // operation row.
        let game_id = self
            .profile_repo
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?
            .game_installation_id;
        ensure_resources_available(
            &*self.operation_repo,
            &self.launch_claims(profile_id, &game_id)?,
            None,
        )?;
        let _resource_lease = self
            .resources
            .try_acquire(&self.launch_claims(profile_id, &game_id)?)?;
        if self.launcher.is_game_running(None) {
            return Err(AppError::game_running(
                "Refusing to launch while a game process is already running",
            ));
        }
        let preflight = self.get_launch_preflight(profile_id, mode)?;
        if !preflight.can_launch {
            return Err(AppError::validation(
                "PREFLIGHT_FAILED",
                preflight.blockers.join("; "),
            ));
        }
        let acknowledged_warnings = match acknowledged {
            Some(reviewed) => {
                if preflight.warnings.iter().any(|w| !reviewed.contains(w)) {
                    return Err(AppError::validation(
                        "LAUNCH_WARNINGS_CHANGED",
                        "The warnings changed since you reviewed them. Review them again before starting.",
                    ));
                }
                preflight.warnings.clone()
            }
            None => Vec::new(),
        };

        let profile = self.profile_repo.get_profile(profile_id)?.unwrap();
        let game = self
            .game_repo
            .get_game(&profile.game_installation_id)?
            .unwrap();

        // The launch advertises this profile's mods directory through
        // `--mods-path`; recording it on the baseline is what lets SMAPI's
        // "Mods go here" line be matched to this session's profile.
        // A runtime test points SMAPI at an empty manager-owned folder, so the
        // profile's own Mods folder is neither read nor changed.
        let mods_path = if mode == LaunchMode::RuntimeTest {
            self.deployment.prepare_empty_mods_root()?
        } else {
            self.deployment.get_profile_mods_root(profile_id)
        };
        let mut baseline = self.log_reader.capture_baseline().ok();
        if let Some(ref mut captured) = baseline {
            captured.expected_mods_path = Some(mods_path.clone());
        }
        let baseline_captured = baseline.is_some();
        let baseline_time = baseline.as_ref().map(|b| b.launch_time);

        // The executable name, its extension, the working directory and the mod
        // isolation argument are all platform facts, so the runtime owns them
        // and the launch service only supplies the profile's mods directory.
        let spec = self
            .runtime
            .build_launch_spec(&game, mode, Some(mods_path.as_path()))?;

        // What the session starts on, observed before the process exists so
        // the game cannot change it first. An observation that fails is
        // recorded as unknown.
        let runtime = self
            .observer
            .as_ref()
            .and_then(|observer| observer.observe(&game.id).ok());
        // Collect expected mod IDs
        let mut expected_mod_ids = Vec::new();
        let components = if mode == LaunchMode::RuntimeTest {
            Vec::new()
        } else {
            self.deployment_repo.list_profile_components(profile_id)?
        };
        for pc in components {
            if pc.enabled {
                if let Some(comp) = self
                    .package_repo
                    .get_package_component(&pc.package_component_id)?
                {
                    expected_mod_ids.push(comp.unique_id);
                }
            }
        }

        // The session exists before the process does, so a spawn failure is
        // recorded against it rather than lost, and its identity (profile,
        // game, versions, mode) is fixed before anything starts.
        let mut session = LaunchSession {
            id: LaunchSessionId::new(),
            game_installation_id: game.id,
            profile_id: *profile_id,
            launch_mode: mode,
            launched_at: Utc::now(),
            ended_at: None,
            pid: None,
            process_identity: None,
            runtime,
            acknowledged_warnings,
            state: SessionState::Starting,
            expected_mod_ids,
            log_baseline_time: baseline_time,
            log_baseline: baseline,
            verification_result: None,
        };
        self.session_repo.save_launch_session(&session)?;

        let identity = match self.launcher.launch_game(&spec) {
            Ok(identity) => identity,
            Err(error) => {
                // The process never started: that is a launch failure, distinct
                // from a game that started and then failed.
                session.state = SessionState::Failed;
                session.ended_at = Some(Utc::now());
                session.verification_result = Some(VerificationResult {
                    confirmed_mods: Vec::new(),
                    details: format!("The game could not be started: {}", error.summary),
                    timestamp: Utc::now(),
                });
                let _ = self.session_repo.save_launch_session(&session);
                return Err(error);
            }
        };

        self.seen_running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.id);
        session.pid = Some(identity.pid);
        // The identity is what makes this session's process recognisable after
        // the manager restarts, when no in-memory tracking survives.
        session.process_identity = Some(identity);
        // Spawning a process is never evidence that mods loaded, and without a
        // log baseline that evidence can never arrive for this session.
        session.state = if baseline_captured {
            SessionState::RunningUnverified
        } else {
            SessionState::VerificationUnavailable
        };
        self.session_repo.save_launch_session(&session)?;

        Ok(Self::session_to_dto(&session))
    }

    pub fn poll_session(&self, id: &LaunchSessionId) -> AppResult<Option<LaunchSessionDto>> {
        let mut session = match self.session_repo.get_launch_session(id)? {
            Some(s) => s,
            None => return Ok(None),
        };

        if matches!(
            session.state,
            SessionState::Exited | SessionState::Failed | SessionState::Interrupted
        ) {
            return Ok(Some(Self::session_to_dto(&session)));
        }

        let recorded_state = self.recorded_session_state(&session);
        let is_running = recorded_state != RecordedProcessState::Exited;

        // Verify session against baseline if available
        if let Some(ref baseline) = session.log_baseline {
            let mut expected_mods = Vec::new();
            for pc in self
                .deployment_repo
                .list_profile_components(&session.profile_id)?
            {
                if !pc.enabled {
                    continue;
                }
                if let Some(comp) = self
                    .package_repo
                    .get_package_component(&pc.package_component_id)?
                {
                    // Only components that were enabled when the session started can
                    // appear in this session's log.
                    if session.expected_mod_ids.contains(&comp.unique_id) {
                        expected_mods.push(ExpectedMod {
                            unique_id: comp.unique_id,
                            name: comp.name,
                            version: comp.version,
                        });
                    }
                }
            }

            if let Ok(verif) = self.log_reader.verify_session(baseline, &expected_mods) {
                if verif.session_matched {
                    if verif.all_mods_confirmed {
                        session.state = SessionState::ModLoadConfirmed;
                    }
                    session.verification_result = Some(VerificationResult {
                        confirmed_mods: verif
                            .verified_mods
                            .into_iter()
                            .filter(|m| m.found_in_log)
                            .map(|m| m.unique_id)
                            .collect(),
                        details: verif
                            .error_details
                            .unwrap_or_else(|| "All expected mods confirmed in log".to_string()),
                        timestamp: Utc::now(),
                    });
                }
            }
        }

        let confirmed = session.state == SessionState::ModLoadConfirmed;
        if confirmed && session.launch_mode == LaunchMode::Modded {
            // Mods loaded with these versions, so this is what "worked" means.
            // A failure to record it only loses a later warning.
            if let Some(observer) = &self.observer {
                if let Ok(versions) = observer.observe(&session.game_installation_id) {
                    let _ = observer.remember(&session.profile_id, &versions);
                    if let Some(known_good) = &self.known_good {
                        let _ = known_good.record(&session.profile_id, &versions);
                    }
                }
            }
        }

        // `recorded_session_state` above marks every session it finds alive.
        let seen_before = self
            .seen_running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&session.id);
        if !is_running && !seen_before {
            // Gone before this run of the manager ever saw it: it ended while
            // the manager was closed, at a time and in a way nobody observed.
            session.state = SessionState::Interrupted;
        } else if !is_running {
            session.ended_at = Some(Utc::now());
            // The log matched this session, so keep a copy before the next
            // start overwrites it. Losing the copy only loses later
            // comparisons.
            if session.verification_result.is_some() {
                if let Some(archive) = &self.log_archive {
                    if let Ok(content) = self.log_reader.read_log_content() {
                        let _ = archive.save(&session.id.to_string(), &content);
                        let _ = archive.prune(SESSION_LOGS_KEPT);
                    }
                }
            }
            // Only a session whose mods were confirmed loaded is a clean exit; a
            // process that stops before confirmation failed, whatever its exit code.
            session.state = if confirmed {
                SessionState::Exited
            } else {
                SessionState::Failed
            };
        } else if !confirmed
            && session.state != SessionState::VerificationUnavailable
            && Utc::now() - session.launched_at > Duration::seconds(VERIFICATION_TIMEOUT_SECONDS)
        {
            session.state = SessionState::VerificationUnavailable;
        }

        self.session_repo.update_launch_session(&session)?;
        Ok(Some(Self::session_to_dto(&session)))
    }

    pub fn terminate_game(&self, id: Option<&LaunchSessionId>) -> AppResult<()> {
        let pid = if let Some(sid) = id {
            self.session_repo
                .get_launch_session(sid)?
                .and_then(|s| s.pid)
        } else {
            None
        };
        self.launcher.terminate_game(pid)
    }

    pub fn get_latest_session(
        &self,
        profile_id: Option<&ProfileId>,
    ) -> AppResult<Option<LaunchSessionDto>> {
        let session = self.session_repo.get_latest_launch_session(profile_id)?;
        Ok(session.map(|s| Self::session_to_dto(&s)))
    }

    /// The profile's most recent sessions, newest first, as recorded.
    pub fn recent_sessions(
        &self,
        profile_id: &ProfileId,
        limit: usize,
    ) -> AppResult<Vec<LaunchSessionDto>> {
        Ok(self
            .session_repo
            .list_launch_sessions(profile_id, limit)?
            .iter()
            .map(Self::session_to_dto)
            .collect())
    }

    fn session_to_dto(s: &LaunchSession) -> LaunchSessionDto {
        let state_str = match s.state {
            SessionState::Starting => "starting",
            SessionState::RunningUnverified => "running_unverified",
            SessionState::ModLoadConfirmed => "mod_load_confirmed",
            SessionState::Exited => "exited",
            SessionState::Failed => "failed",
            SessionState::VerificationUnavailable => "verification_unavailable",
            SessionState::Interrupted => "interrupted",
        };

        let verified_mods = s
            .verification_result
            .as_ref()
            .map(|v| v.confirmed_mods.clone())
            .unwrap_or_default();
        let verification_details = s.verification_result.as_ref().map(|v| v.details.clone());

        LaunchSessionDto {
            id: s.id.to_string(),
            profile_id: s.profile_id.to_string(),
            launch_mode: match s.launch_mode {
                LaunchMode::Modded => "modded",
                LaunchMode::Vanilla => "vanilla",
                LaunchMode::RuntimeTest => "runtime_test",
            }
            .to_string(),
            state: state_str.to_string(),
            launched_at: s.launched_at.to_rfc3339(),
            ended_at: s.ended_at.map(|t| t.to_rfc3339()),
            pid: s.pid,
            verified_mods,
            verification_details,
            acknowledged_warnings: s.acknowledged_warnings.clone(),
            expected_mods: s.expected_mod_ids.iter().map(|id| id.to_string()).collect(),
            game_version: s.runtime.as_ref().and_then(|r| r.game_version.clone()),
            smapi_version: s.runtime.as_ref().and_then(|r| r.smapi_version.clone()),
        }
    }
}
