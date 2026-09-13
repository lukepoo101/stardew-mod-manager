use crate::api::dto::LaunchSessionDto;
use crate::error::{AppError, AppResult};
use crate::ports::deployment::DeploymentPort;
use crate::ports::launcher::GameLauncherPort;
use crate::ports::logging::SessionLogPort;
use crate::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, LaunchSessionRepository, OperationRepository,
    PackageCatalogRepository, ProfileRepository, SmapiRepository,
};
use chrono::{Duration, Utc};
use manager_core::dependency::{build_dependency_graph, DependencyEdgeType};
use manager_core::ids::{LaunchSessionId, ProfileId};
use manager_core::launch::{
    LaunchMode, LaunchSession, LaunchSpec, PreflightCheck, SessionState, VerificationResult,
};
use std::sync::Arc;

/// How long a running session may go without load evidence before verification
/// is reported as unavailable rather than silently pending.
const VERIFICATION_TIMEOUT_SECONDS: i64 = 120;

pub struct LaunchService {
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
}

impl LaunchService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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
    ) -> Self {
        Self {
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
        }
    }

    pub fn get_launch_preflight(
        &self,
        profile_id: &ProfileId,
        mode: LaunchMode,
    ) -> AppResult<PreflightCheck> {
        let mut blockers = Vec::new();
        let warnings = Vec::new();

        let profile = self
            .profile_repo
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;

        let game = self
            .game_repo
            .get_game(&profile.game_installation_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game installation not found"))?;

        if mode != LaunchMode::Vanilla
            && self.smapi_repo.get_smapi_installation(&game.id)?.is_none()
        {
            blockers.push("SMAPI is not installed for this game installation".to_string());
        }

        let unresolved = self
            .operation_repo
            .list_unresolved_resources_for_profile(profile_id)?;
        if !unresolved.is_empty() {
            blockers.push("This profile has an unresolved or recovering operation".to_string());
        }

        if mode != LaunchMode::Vanilla {
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

            let graph = build_dependency_graph(&manifests, None);
            for edge in &graph.edges {
                let required = edge.edge_type == DependencyEdgeType::Required
                    || edge.edge_type == DependencyEdgeType::ContentPackFor;
                if required && graph.get_node(&edge.target_id).is_none() {
                    blockers.push(format!(
                        "'{}' requires '{}', which is not installed in this profile",
                        edge.source_id, edge.target_id
                    ));
                }
            }
        }

        // Check if game is already running
        if let Some(latest) = self
            .session_repo
            .get_latest_launch_session(Some(profile_id))?
        {
            if (latest.state == SessionState::Starting
                || latest.state == SessionState::RunningUnverified
                || latest.state == SessionState::ModLoadConfirmed)
                && self.launcher.is_game_running(latest.pid)
            {
                blockers.push("Game is already running".to_string());
            }
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
        let preflight = self.get_launch_preflight(profile_id, mode)?;
        if !preflight.can_launch {
            return Err(AppError::validation(
                "PREFLIGHT_FAILED",
                preflight.blockers.join("; "),
            ));
        }

        let profile = self.profile_repo.get_profile(profile_id)?.unwrap();
        let game = self
            .game_repo
            .get_game(&profile.game_installation_id)?
            .unwrap();

        let baseline = self.log_reader.capture_baseline().ok();
        let baseline_captured = baseline.is_some();
        let baseline_time = baseline.as_ref().map(|b| b.launch_time);

        let mods_path = self.deployment.get_profile_mods_root(profile_id);
        let executable = match mode {
            LaunchMode::Modded | LaunchMode::RuntimeTest => {
                game.canonical_root.join("StardewModdingAPI")
            }
            LaunchMode::Vanilla => game.canonical_root.join("StardewValley"),
        };

        let mut args = Vec::new();
        if mode == LaunchMode::Modded || mode == LaunchMode::RuntimeTest {
            args.push("--mods-path".to_string());
            args.push(mods_path.to_string_lossy().to_string());
        }

        let spec = LaunchSpec {
            executable,
            args,
            working_dir: game.canonical_root.clone(),
            env: Vec::new(),
        };

        let pid = self.launcher.launch_game(&spec)?;

        // Collect expected mod IDs
        let mut expected_mod_ids = Vec::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            if pc.enabled {
                if let Some(comp) = self
                    .package_repo
                    .get_package_component(&pc.package_component_id)?
                {
                    expected_mod_ids.push(comp.unique_id);
                }
            }
        }

        let session_id = LaunchSessionId::new();
        let session = LaunchSession {
            id: session_id,
            game_installation_id: game.id,
            profile_id: *profile_id,
            launch_mode: mode,
            launched_at: Utc::now(),
            ended_at: None,
            pid: Some(pid),
            // Spawning a process is never evidence that mods loaded, and without a
            // log baseline that evidence can never arrive for this session.
            state: if baseline_captured {
                SessionState::RunningUnverified
            } else {
                SessionState::VerificationUnavailable
            },
            expected_mod_ids,
            log_baseline_time: baseline_time,
            log_baseline: baseline,
            verification_result: None,
        };

        self.session_repo.save_launch_session(&session)?;

        Ok(Self::session_to_dto(&session))
    }

    pub fn poll_session(&self, id: &LaunchSessionId) -> AppResult<Option<LaunchSessionDto>> {
        let mut session = match self.session_repo.get_launch_session(id)? {
            Some(s) => s,
            None => return Ok(None),
        };

        if session.state == SessionState::Exited || session.state == SessionState::Failed {
            return Ok(Some(Self::session_to_dto(&session)));
        }

        let is_running = self.launcher.is_game_running(session.pid);

        // Verify session against baseline if available
        if let Some(ref baseline) = session.log_baseline {
            let mut expected_pairs = Vec::new();
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
                        expected_pairs.push((comp.unique_id, comp.version));
                    }
                }
            }

            if let Ok(verif) = self.log_reader.verify_session(baseline, &expected_pairs) {
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

        if !is_running {
            session.ended_at = Some(Utc::now());
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

    fn session_to_dto(s: &LaunchSession) -> LaunchSessionDto {
        let state_str = match s.state {
            SessionState::Starting => "starting",
            SessionState::RunningUnverified => "running_unverified",
            SessionState::ModLoadConfirmed => "mod_load_confirmed",
            SessionState::Exited => "exited",
            SessionState::Failed => "failed",
            SessionState::VerificationUnavailable => "verification_unavailable",
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
            state: state_str.to_string(),
            launched_at: s.launched_at.to_rfc3339(),
            ended_at: s.ended_at.map(|t| t.to_rfc3339()),
            pid: s.pid,
            verified_mods,
            verification_details,
        }
    }
}
