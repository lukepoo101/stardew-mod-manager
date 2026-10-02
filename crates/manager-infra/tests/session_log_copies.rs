//! A session's saved SMAPI log is what Diagnostics reads for that session.

use chrono::Utc;
use manager_app::ports::logging::SessionLogArchivePort;
use manager_app::ports::repositories::{
    GameInstallationRepository, LaunchSessionRepository, ProfileRepository,
};
use manager_app::services::{DiagnosticsService, HostEnvironment};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{GameInstallationId, LaunchSessionId};
use manager_core::launch::{LaunchMode, LaunchSession, SessionState};
use manager_core::profile::Profile;
use manager_infra::db::SqliteStateRepository;
use manager_infra::log_reader::SmapiSessionLogReader;
use manager_infra::session_logs::FilesystemSessionLogs;
use std::sync::Arc;

#[test]
fn an_earlier_session_is_read_from_its_saved_log() {
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
    let profile = Profile::new(game.id, "Main");
    repo.save_profile(&profile).unwrap();
    let session = LaunchSession {
        id: LaunchSessionId::new(),
        game_installation_id: game.id,
        profile_id: profile.id,
        launch_mode: LaunchMode::Modded,
        launched_at: Utc::now(),
        ended_at: Some(Utc::now()),
        pid: None,
        process_identity: None,
        runtime: None,
        acknowledged_warnings: Vec::new(),
        state: SessionState::Exited,
        expected_mod_ids: Vec::new(),
        log_baseline_time: None,
        log_baseline: None,
        verification_result: None,
    };
    repo.save_launch_session(&session).unwrap();

    // SMAPI's current log belongs to a later run.
    let current = tmp.path().join("SMAPI-latest.txt");
    std::fs::write(&current, "[10:00:00 ERROR Later] something else\n").unwrap();
    let archive = Arc::new(FilesystemSessionLogs::new(tmp.path().join("session-logs")));
    archive
        .save(
            &session.id.to_string(),
            "[09:00:00 ERROR Earlier] the old problem\n",
        )
        .unwrap();

    let diagnostics = DiagnosticsService::new(
        repo.clone(),
        Arc::new(SmapiSessionLogReader::new(Some(current))),
        HostEnvironment {
            operating_system: OperatingSystem::Linux,
            app_data_dir: tmp.path().into(),
            cache_dir: tmp.path().into(),
            steam_roots: Vec::new(),
        },
    )
    .with_log_archive(archive);

    let earlier = diagnostics.get_diagnostics(Some(&session.id)).unwrap();
    assert!(earlier.log_is_saved_copy);
    assert!(earlier.raw_log.contains("the old problem"));
    assert_eq!(earlier.log_match, "current_session");

    // Without a session the current file is read, as before.
    let latest = diagnostics.get_diagnostics(None).unwrap();
    assert!(!latest.log_is_saved_copy);
    assert!(latest.raw_log.contains("something else"));
}
