use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::launcher::GameLauncherPort;
use manager_app::ports::logging::SessionLogPort;
use manager_app::ports::repositories::{
    GameInstallationRepository, LaunchSessionRepository, ProfileRepository, SmapiRepository,
};
use manager_app::services::LaunchService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::ModUniqueId;
use manager_core::launch::{
    LaunchMode, LaunchSpec, SessionState, SessionVerificationBaseline, SessionVerificationResult,
};
use manager_core::profile::Profile;
use manager_core::smapi::ManagedSmapiInstallation;
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::paths::AppPaths;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct FakeLauncher {
    running: AtomicBool,
}

impl GameLauncherPort for FakeLauncher {
    fn launch_game(&self, _spec: &LaunchSpec) -> AppResult<u32> {
        self.running.store(true, Ordering::SeqCst);
        Ok(4242)
    }

    fn is_game_running(&self, _pid: Option<u32>) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn terminate_game(&self, _pid: Option<u32>) -> AppResult<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

struct FakeLog {
    baseline_available: bool,
}

impl SessionLogPort for FakeLog {
    fn capture_baseline(&self) -> AppResult<SessionVerificationBaseline> {
        if !self.baseline_available {
            return Err(manager_app::error::AppError::filesystem(
                "No SMAPI log",
                "log file missing",
            ));
        }
        Ok(SessionVerificationBaseline {
            log_path: PathBuf::from("/tmp/SMAPI-latest.txt"),
            initial_mtime: None,
            initial_size: 0,
            launch_time: Utc::now(),
            initial_inode: None,
            expected_mods_path: None,
        })
    }

    fn verify_session(
        &self,
        _baseline: &SessionVerificationBaseline,
        _expected_mods: &[(ModUniqueId, String)],
    ) -> AppResult<SessionVerificationResult> {
        Ok(SessionVerificationResult {
            session_matched: false,
            session_start_time: None,
            custom_mods_path_detected: false,
            all_mods_confirmed: false,
            verified_mods: Vec::new(),
            error_details: Some("No session evidence yet".to_string()),
        })
    }

    fn read_log_content(&self) -> AppResult<String> {
        Ok(String::new())
    }

    fn log_file_path(&self) -> PathBuf {
        PathBuf::from("/tmp/SMAPI-latest.txt")
    }
}

struct Harness {
    service: LaunchService,
    launcher: Arc<FakeLauncher>,
    repo: Arc<SqliteStateRepository>,
    profile_id: manager_core::ids::ProfileId,
    _tmp: tempfile::TempDir,
}

fn harness(baseline_available: bool) -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("state.sqlite3")).unwrap());

    let game = GameInstallation {
        id: manager_core::ids::GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();

    let profile = Profile::new(game.id, "Default");
    repo.save_profile(&profile).unwrap();

    repo.save_smapi_installation(&ManagedSmapiInstallation {
        game_installation_id: game.id,
        release_version: "4.1.10".to_string(),
        release_policy_id: "pinned".to_string(),
        installed_at: Utc::now(),
    })
    .unwrap();

    let launcher = Arc::new(FakeLauncher {
        running: AtomicBool::new(false),
    });
    let deployment = Arc::new(FilesystemDeploymentAdapter::new(AppPaths::new(
        tmp.path().join("data"),
        tmp.path().join("cache"),
    )));

    let service = LaunchService::new(
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        launcher.clone(),
        deployment,
        Arc::new(FakeLog { baseline_available }),
    );

    Harness {
        service,
        launcher,
        repo,
        profile_id: profile.id,
        _tmp: tmp,
    }
}

#[test]
fn spawning_a_process_is_not_treated_as_a_verified_session() {
    let h = harness(true);
    let session = h
        .service
        .launch_profile(&h.profile_id, LaunchMode::Modded)
        .unwrap();
    assert_eq!(session.state, "running_unverified");
}

#[test]
fn a_session_without_a_log_baseline_is_immediately_unverifiable() {
    let h = harness(false);
    let session = h
        .service
        .launch_profile(&h.profile_id, LaunchMode::Modded)
        .unwrap();
    assert_eq!(session.state, "verification_unavailable");
}

#[test]
fn a_process_that_stops_before_mod_load_confirmation_failed() {
    let h = harness(true);
    let session = h
        .service
        .launch_profile(&h.profile_id, LaunchMode::Modded)
        .unwrap();
    let session_id = session.id.parse().unwrap();

    h.launcher.terminate_game(None).unwrap();

    let polled = h.service.poll_session(&session_id).unwrap().unwrap();
    assert_eq!(polled.state, "failed");
    assert!(polled.ended_at.is_some());
}

#[test]
fn a_confirmed_session_that_later_stops_has_exited() {
    let h = harness(true);
    let session = h
        .service
        .launch_profile(&h.profile_id, LaunchMode::Modded)
        .unwrap();
    let session_id: manager_core::ids::LaunchSessionId = session.id.parse().unwrap();

    let mut stored = LaunchSessionRepository::get_launch_session(&*h.repo, &session_id)
        .unwrap()
        .unwrap();
    stored.state = SessionState::ModLoadConfirmed;
    LaunchSessionRepository::update_launch_session(&*h.repo, &stored).unwrap();

    h.launcher.terminate_game(None).unwrap();

    let polled = h.service.poll_session(&session_id).unwrap().unwrap();
    assert_eq!(polled.state, "exited");
}
