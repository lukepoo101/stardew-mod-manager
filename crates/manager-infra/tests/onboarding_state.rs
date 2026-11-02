//! Whether guided setup still applies after migrations, for a brand-new
//! database and for databases written by earlier versions.

use chrono::Utc;
use manager_app::ports::repositories::GameInstallationRepository;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::GameInstallationId;
use manager_core::profile::OnboardingDisposition;
use manager_infra::db::migrations::run_migrations;
use manager_infra::db::SqliteStateRepository;
use rusqlite::Connection;

fn game(root: &std::path::Path) -> GameInstallation {
    GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: root.to_path_buf(),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Manual,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    }
}

#[test]
fn a_fresh_database_has_not_finished_setup_and_still_has_not_after_a_restart_mid_wizard() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db).unwrap();
    assert_eq!(
        repo.get_app_context().unwrap().onboarding_disposition,
        OnboardingDisposition::NotStarted
    );

    // The first wizard step registers the game and makes it active.
    let registered = game(&tmp.path().join("Stardew Valley"));
    repo.save_game(&registered).unwrap();
    let mut ctx = repo.get_app_context().unwrap();
    ctx.active_game_installation_id = Some(registered.id);
    repo.save_app_context(&ctx).unwrap();
    drop(repo);

    // Restarting before SMAPI setup must not count as finishing setup.
    let reopened = SqliteStateRepository::new(&db).unwrap();
    let ctx = reopened.get_app_context().unwrap();
    assert_eq!(ctx.onboarding_disposition, OnboardingDisposition::NotStarted);
    assert_eq!(ctx.active_game_installation_id, Some(registered.id));
}

/// A database as the previous release left it: every migration up to 0009
/// applied, so 0003's seed already wrote "completed".
fn previous_release_database(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    run_migrations(&conn).unwrap();
    conn.execute_batch(
        "DELETE FROM schema_migrations WHERE version >= 10;
         UPDATE app_context SET onboarding_disposition = 'completed';",
    )
    .unwrap();
    conn
}

#[test]
fn an_upgraded_database_with_a_game_keeps_its_finished_setup() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("state.sqlite3");
    let conn = previous_release_database(&db);
    conn.execute(
        "INSERT INTO game_installations (id, canonical_root, platform_kind, detected_version, validated_at, is_fresh, operating_system, storefront, management_mode, created_at)
         VALUES ('5f0c0e0a-0000-4000-8000-000000000001', '/games/Stardew Valley', 'steam_native', '1.6.15', '2026-01-01T00:00:00Z', 1, 'linux', 'steam', 'managed', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    drop(conn);

    let repo = SqliteStateRepository::new(&db).unwrap();
    assert_eq!(
        repo.get_app_context().unwrap().onboarding_disposition,
        OnboardingDisposition::Completed
    );
}

#[test]
fn an_upgraded_database_that_never_registered_a_game_offers_setup_again() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("state.sqlite3");
    drop(previous_release_database(&db));

    let repo = SqliteStateRepository::new(&db).unwrap();
    assert_eq!(
        repo.get_app_context().unwrap().onboarding_disposition,
        OnboardingDisposition::NotStarted
    );
}

#[test]
fn skipping_or_finishing_setup_is_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db).unwrap();
    let mut ctx = repo.get_app_context().unwrap();
    ctx.onboarding_disposition = OnboardingDisposition::Skipped;
    repo.save_app_context(&ctx).unwrap();
    drop(repo);
    assert_eq!(
        SqliteStateRepository::new(&db)
            .unwrap()
            .get_app_context()
            .unwrap()
            .onboarding_disposition,
        OnboardingDisposition::Skipped
    );
}
