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
fn a_registered_game_round_trips_through_the_modern_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = SqliteStateRepository::new(tmp.path().join("state.db")).unwrap();
    let id = GameInstallationId::new();

    GameInstallationRepository::save_game(
        &repo,
        &manager_core::game::GameInstallation {
            id,
            canonical_root: tmp.path().join("Stardew Valley"),
            operating_system: manager_core::game::OperatingSystem::Linux,
            storefront: manager_core::game::Storefront::Steam,
            management_mode: manager_core::game::ManagementMode::Managed,
            created_at: chrono::Utc::now(),
        },
    )
    .unwrap();

    let game = GameInstallationRepository::get_game(&repo, &id)
        .unwrap()
        .expect("a registered game is visible to the modern repository");
    assert_eq!(game.id, id);
}

#[test]
fn legacy_profile_storage_moves_with_the_migrated_identifier() {
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let db_path = data_dir.join("state.sqlite3");
    std::fs::create_dir_all(&data_dir).unwrap();

    let legacy_mods = data_dir.join("setups").join("setup-abc").join("Mods");
    std::fs::create_dir_all(legacy_mods.join("Author.Mod")).unwrap();
    std::fs::write(legacy_mods.join("Author.Mod").join("manifest.json"), "{}").unwrap();

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

    SqliteStateRepository::new(&db_path).unwrap();

    let migrated = ProfileId::from_uuid(manager_core::ids::derive_uuid("setup-abc"));
    let paths = AppPaths::new(data_dir.clone(), tmp.path().join("cache"));
    assert!(paths
        .profile_mods_dir(&migrated)
        .join("Author.Mod")
        .join("manifest.json")
        .exists());
    assert!(!data_dir.join("setups").join("setup-abc").exists());
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

#[test]
fn disabling_a_deployment_removes_it_from_the_game_visible_mods_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let adapter = FilesystemDeploymentAdapter::new(paths.clone());
    let profile_id = ProfileId::new();

    let staged = tmp.path().join("staged");
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::write(staged.join("manifest.json"), "{}").unwrap();
    adapter
        .publish_deployment(&profile_id, &staged, "Author.Mod")
        .unwrap();

    let deployed = paths.profile_mods_dir(&profile_id).join("Author.Mod");
    assert!(deployed.exists());

    adapter
        .disable_deployment(&profile_id, "Author.Mod")
        .unwrap();
    assert!(!deployed.exists());
    assert!(paths
        .profile_disabled_dir(&profile_id)
        .join("Author.Mod")
        .join("manifest.json")
        .exists());

    adapter
        .enable_deployment(&profile_id, "Author.Mod")
        .unwrap();
    assert!(deployed.join("manifest.json").exists());
    assert!(!paths
        .profile_disabled_dir(&profile_id)
        .join("Author.Mod")
        .exists());
}

#[test]
fn a_corrupt_stored_artifact_is_replaced_instead_of_reused() {
    use manager_app::ports::artifacts::ArtifactStorePort;

    let tmp = tempfile::tempdir().unwrap();
    let packages = tmp.path().join("packages");
    let store = manager_infra::package_store::FilesystemPackageStore::new(&packages);

    let source = tmp.path().join("mod.zip");
    std::fs::write(&source, b"real archive bytes").unwrap();

    let artifact = store.store_artifact(&source).unwrap();
    let stored = packages.join(format!("{}.zip", artifact.hash.as_str()));
    std::fs::write(&stored, b"corrupted").unwrap();

    let reused = store.store_artifact(&source).unwrap();
    assert_eq!(reused.hash, artifact.hash);
    assert_eq!(
        std::fs::read(&stored).unwrap(),
        b"real archive bytes".to_vec()
    );
}
