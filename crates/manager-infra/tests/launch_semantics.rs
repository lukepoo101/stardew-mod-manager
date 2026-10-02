use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::launcher::{GameLauncherPort, RecordedProcessState};
use manager_app::ports::logging::{ExpectedMod, SessionLogPort};
use manager_app::ports::repositories::{
    GameInstallationRepository, LaunchSessionRepository, ProfileRepository, SmapiRepository,
};
use manager_app::services::LaunchService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::launch::{
    LaunchMode, LaunchSpec, ProcessIdentity, SessionState, SessionVerificationBaseline,
    SessionVerificationResult,
};
use manager_core::ports::InstanceLock;
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
    fail_spawn: AtomicBool,
    last_spec: std::sync::Mutex<Option<LaunchSpec>>,
}

struct NoopLock;
impl InstanceLock for NoopLock {
    fn acquire_guard(&self) -> Result<Box<dyn std::any::Any + Send + Sync>, String> {
        Ok(Box::new(()))
    }
}

impl GameLauncherPort for FakeLauncher {
    fn launch_game(&self, spec: &LaunchSpec) -> AppResult<ProcessIdentity> {
        *self.last_spec.lock().unwrap() = Some(spec.clone());
        if self.fail_spawn.load(Ordering::SeqCst) {
            return Err(manager_app::error::AppError::validation(
                "SPAWN_FAILED",
                "The executable could not be run",
            ));
        }
        self.running.store(true, Ordering::SeqCst);
        Ok(ProcessIdentity::new(4242, Some(1), None))
    }

    fn is_game_running(&self, _pid: Option<u32>) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn identify_recorded(&self, _identity: &ProcessIdentity) -> RecordedProcessState {
        if self.running.load(Ordering::SeqCst) {
            RecordedProcessState::Running
        } else {
            RecordedProcessState::Exited
        }
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
        _expected_mods: &[ExpectedMod],
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

    fn log_is_available(&self) -> bool {
        self.baseline_available
    }
}

struct Harness {
    service: LaunchService,
    launcher: Arc<FakeLauncher>,
    repo: Arc<SqliteStateRepository>,
    profile_id: manager_core::ids::ProfileId,
    data_dir: PathBuf,
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
        fail_spawn: AtomicBool::new(false),
        last_spec: std::sync::Mutex::new(None),
    });
    let deployment = Arc::new(FilesystemDeploymentAdapter::new(AppPaths::new(
        tmp.path().join("data"),
        tmp.path().join("cache"),
    )));

    let service = LaunchService::new(
        Arc::new(manager_app::services::ResourceCoordinator::new()),
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
        Arc::new(NoopLock),
        Arc::new(manager_infra::TestGameRuntime::for_platform(
            manager_core::game::OperatingSystem::Linux,
        )),
    );

    Harness {
        service,
        launcher,
        repo,
        profile_id: profile.id,
        data_dir: tmp.path().join("data"),
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

#[test]
fn a_spawn_failure_is_recorded_as_its_own_failed_session() {
    let h = harness(true);
    h.launcher.fail_spawn.store(true, Ordering::SeqCst);
    let error = h
        .service
        .launch_profile(&h.profile_id, LaunchMode::Modded)
        .unwrap_err();
    assert_eq!(error.code, "SPAWN_FAILED");

    let session = h
        .repo
        .get_latest_launch_session(Some(&h.profile_id))
        .unwrap()
        .expect("the attempt is recorded before the spawn");
    assert_eq!(session.state, SessionState::Failed);
    assert!(session.pid.is_none());
    assert!(session.ended_at.is_some());
    assert!(session
        .verification_result
        .unwrap()
        .details
        .starts_with("The game could not be started"));
    // A failed spawn does not leave anything that blocks the next launch.
    h.launcher.fail_spawn.store(false, Ordering::SeqCst);
    assert!(h
        .service
        .launch_profile(&h.profile_id, LaunchMode::Modded)
        .is_ok());
}

#[test]
fn launching_past_warnings_needs_the_current_ones_reviewed() {
    // Without a log baseline the session is unverifiable, which the next
    // preflight reports as a non-blocking warning.
    let h = harness(false);
    h.service
        .launch_profile(&h.profile_id, LaunchMode::Modded)
        .unwrap();
    h.launcher.running.store(false, Ordering::SeqCst);
    let warnings = h
        .service
        .get_launch_preflight(&h.profile_id, LaunchMode::Modded)
        .unwrap()
        .warnings;
    assert!(!warnings.is_empty());

    // Reviewing nothing (or something else) is not enough.
    let error = h
        .service
        .launch_profile_acknowledging(&h.profile_id, LaunchMode::Modded, Some(&[]))
        .unwrap_err();
    assert_eq!(error.code, "LAUNCH_WARNINGS_CHANGED");

    let session = h
        .service
        .launch_profile_acknowledging(&h.profile_id, LaunchMode::Modded, Some(&warnings))
        .unwrap();
    assert_eq!(session.acknowledged_warnings, warnings);
}

#[test]
fn a_runtime_test_starts_smapi_with_an_empty_folder_and_leaves_the_profile_alone() {
    let h = harness(true);
    let profile_mods = AppPaths::new(h.data_dir.clone(), h.data_dir.join("../cache"))
        .profile_mods_dir(&h.profile_id);
    std::fs::create_dir_all(profile_mods.join("MyMod")).unwrap();

    let session = h
        .service
        .launch_profile(&h.profile_id, LaunchMode::RuntimeTest)
        .unwrap();
    assert!(session.expected_mods.is_empty());

    let spec = h.launcher.last_spec.lock().unwrap().clone().unwrap();
    let at = spec.args.iter().position(|a| a == "--mods-path").unwrap();
    let mods_path = PathBuf::from(&spec.args[at + 1]);
    assert!(mods_path.ends_with("runtime-test/Mods"));
    assert!(std::fs::read_dir(&mods_path).unwrap().next().is_none());
    assert!(profile_mods.join("MyMod").is_dir());
}
