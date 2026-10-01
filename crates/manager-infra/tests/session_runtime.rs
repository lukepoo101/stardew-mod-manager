//! A launch session keeps the game and SMAPI versions it started on, and
//! sessions recorded before that was kept read back as unknown.

use chrono::Utc;
use manager_app::ports::repositories::{
    GameInstallationRepository, LaunchSessionRepository, ProfileRepository,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{GameInstallationId, LaunchSessionId};
use manager_core::launch::{LaunchMode, LaunchSession, RuntimeVersions, SessionState};
use manager_core::profile::Profile;
use manager_infra::db::SqliteStateRepository;

fn session(
    game: GameInstallationId,
    profile: &Profile,
    runtime: Option<RuntimeVersions>,
) -> LaunchSession {
    LaunchSession {
        id: LaunchSessionId::new(),
        game_installation_id: game,
        profile_id: profile.id,
        launch_mode: LaunchMode::Modded,
        launched_at: Utc::now(),
        ended_at: None,
        pid: Some(1),
        process_identity: None,
        runtime,
        acknowledged_warnings: Vec::new(),
        state: SessionState::RunningUnverified,
        expected_mod_ids: Vec::new(),
        log_baseline_time: None,
        log_baseline: None,
        verification_result: None,
    }
}

#[test]
fn runtime_versions_round_trip_and_survive_later_updates() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = SqliteStateRepository::new(tmp.path().join("s.sqlite3")).unwrap();
    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();
    let profile = Profile::new(game.id, "Main");
    repo.save_profile(&profile).unwrap();

    let runtime = RuntimeVersions {
        game_version: Some("1.6.15".into()),
        smapi_version: Some("4.1.10".into()),
    };
    let mut started = session(game.id, &profile, Some(runtime.clone()));
    started.acknowledged_warnings = vec!["The last launch was not verified".into()];
    repo.save_launch_session(&started).unwrap();
    assert_eq!(
        repo.get_launch_session(&started.id)
            .unwrap()
            .unwrap()
            .runtime,
        Some(runtime.clone())
    );

    // Later state changes keep what the session started on.
    started.state = SessionState::Exited;
    started.ended_at = Some(Utc::now());
    started.runtime = None;
    started.acknowledged_warnings.clear();
    repo.save_launch_session(&started).unwrap();
    let ended = repo.get_launch_session(&started.id).unwrap().unwrap();
    assert_eq!(ended.state, SessionState::Exited);
    assert_eq!(ended.runtime, Some(runtime));
    assert_eq!(
        ended.acknowledged_warnings,
        vec!["The last launch was not verified".to_string()]
    );

    let unknown = session(game.id, &profile, None);
    repo.save_launch_session(&unknown).unwrap();
    assert_eq!(
        repo.get_launch_session(&unknown.id)
            .unwrap()
            .unwrap()
            .runtime,
        None
    );
}
