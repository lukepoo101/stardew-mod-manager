//! Switching the active profile waits for the game to close and for any
//! change to either profile to finish; a prepared preview does not block it.

use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::launcher::{GameLauncherPort, RecordedProcessState};
use manager_app::ports::repositories::{
    GameInstallationRepository, OperationRepository, ProfileRepository,
};
use manager_app::services::ProfilesService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{GameInstallationId, OperationId};
use manager_core::launch::{LaunchSpec, ProcessIdentity};
use manager_core::operation::{Operation, OperationKind, OperationState};
use manager_core::profile::Profile;
use manager_infra::db::SqliteStateRepository;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct Launcher(AtomicBool);
impl GameLauncherPort for Launcher {
    fn launch_game(&self, _: &LaunchSpec) -> AppResult<ProcessIdentity> {
        unreachable!()
    }
    fn is_game_running(&self, _: Option<u32>) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    fn identify_recorded(&self, _: &ProcessIdentity) -> RecordedProcessState {
        RecordedProcessState::Exited
    }
    fn terminate_game(&self, _: Option<u32>) -> AppResult<()> {
        Ok(())
    }
}

struct Fixture {
    repo: Arc<SqliteStateRepository>,
    launcher: Arc<Launcher>,
    service: ProfilesService,
    game: GameInstallationId,
    main: Profile,
    other: Profile,
    _tmp: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("s.sqlite3")).unwrap());
    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();
    let main = Profile::new(game.id, "Main");
    let other = Profile::new(game.id, "Other");
    repo.save_profile(&main).unwrap();
    repo.save_profile(&other).unwrap();
    let launcher = Arc::new(Launcher(AtomicBool::new(false)));
    let service = ProfilesService::new(repo.clone(), repo.clone(), repo.clone())
        .with_switch_guards(repo.clone(), launcher.clone());
    service.switch_active_profile(&game.id, &main.id).unwrap();
    Fixture {
        repo,
        launcher,
        service,
        game: game.id,
        main,
        other,
        _tmp: tmp,
    }
}

fn operation(f: &Fixture, state: OperationState) {
    let now = Utc::now();
    f.repo
        .create_operation(&Operation {
            id: OperationId::new(),
            kind: OperationKind::ModInstall,
            state,
            game_installation_id: Some(f.game),
            profile_id: Some(f.main.id),
            expected_profile_revision: None,
            plan_schema_version: 1,
            plan_json: "{}".into(),
            progress_current: None,
            progress_total: None,
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
        .unwrap();
}

#[test]
fn a_running_game_blocks_switching() {
    let f = fixture();
    f.launcher.0.store(true, Ordering::SeqCst);
    let error = f
        .service
        .switch_active_profile(&f.game, &f.other.id)
        .unwrap_err();
    assert_eq!(error.code, "GAME_RUNNING");
}

#[test]
fn an_unfinished_change_blocks_switching_but_a_preview_does_not() {
    let f = fixture();
    operation(&f, OperationState::Prepared);
    f.service
        .switch_active_profile(&f.game, &f.other.id)
        .unwrap();
    f.service
        .switch_active_profile(&f.game, &f.main.id)
        .unwrap();

    operation(&f, OperationState::RecoveryRequired);
    let error = f
        .service
        .switch_active_profile(&f.game, &f.other.id)
        .unwrap_err();
    assert_eq!(error.code, "PROFILE_SWITCH_BLOCKED");
}
