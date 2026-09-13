use manager_app::ports::deployment::DeploymentPort;
use manager_app::ports::repositories::{
    GameInstallationRepository, PreferencesRepository, ProfileRepository,
};
use manager_core::ids::{GameInstallationId, OperationId, ProfileId};
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::paths::AppPaths;

#[test]
fn preferences_round_trip_per_key() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = SqliteStateRepository::new(tmp.path().join("state.db")).unwrap();

    assert_eq!(repo.get_preference("theme").unwrap(), None);

    repo.set_preference("theme", "dark").unwrap();
    repo.set_preference("last_profile", "profile-1").unwrap();
    repo.set_preference("theme", "light").unwrap();

    assert_eq!(
        repo.get_preference("theme").unwrap(),
        Some("light".to_string())
    );
    assert_eq!(
        repo.get_preference("last_profile").unwrap(),
        Some("profile-1".to_string())
    );
}

#[test]
fn legacy_string_ids_are_readable_by_the_modern_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("legacy.sqlite3");

    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (1, '2026-01-01T00:00:00Z');",
            include_str!("../migrations/0001_initial.sql")
        ))
        .unwrap();
        conn.execute(
            "INSERT INTO game_installations (id, canonical_root, platform_kind, detected_version, validated_at, is_fresh)
             VALUES ('game-abc', '/games/Stardew Valley', 'steam_native', '1.6.8', '2026-01-01T00:00:00Z', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO setups (id, game_id, display_name, relative_mods_dir, created_at)
             VALUES ('setup-abc', 'game-abc', 'Default', 'Mods', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    }

    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let games = GameInstallationRepository::list_games(&repo).unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(
        games[0].id,
        GameInstallationId::from_uuid(manager_core::ids::derive_uuid("game-abc"))
    );

    let profiles = repo.list_profiles(&games[0].id).unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(
        profiles[0].id,
        ProfileId::from_uuid(manager_core::ids::derive_uuid("setup-abc"))
    );
}

#[test]
fn deployment_paths_cannot_escape_the_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let adapter = FilesystemDeploymentAdapter::new(AppPaths::new(
        tmp.path().join("data"),
        tmp.path().join("cache"),
    ));
    let profile_id = ProfileId::new();
    let operation_id = OperationId::new();

    let staged = tmp.path().join("staged");
    std::fs::create_dir_all(&staged).unwrap();

    for escape in ["../escaped", "/etc/escaped", "nested/../../escaped"] {
        assert!(adapter
            .publish_deployment(&profile_id, &staged, escape)
            .is_err());
        assert!(adapter
            .quarantine_deployment(&profile_id, &operation_id, escape)
            .is_err());
        assert!(adapter
            .restore_quarantined_deployment(&profile_id, &operation_id, escape)
            .is_err());
    }

    assert!(!tmp.path().join("data").join("escaped").exists());
    assert!(adapter
        .publish_deployment(&profile_id, &staged, "Author.Mod")
        .is_ok());
}
