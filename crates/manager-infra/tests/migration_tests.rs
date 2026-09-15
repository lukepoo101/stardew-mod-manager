use manager_infra::db::migrations::run_migrations;
use manager_infra::SqliteStateRepository;
use rusqlite::Connection;
use tempfile::tempdir;

#[test]
fn test_fresh_database_runs_all_migrations() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("fresh.sqlite3");

    let conn = Connection::open(&db_path).unwrap();
    run_migrations(&conn).expect("Migrations on fresh db should succeed");

    // Verify all migrations are recorded
    let versions: Vec<u32> = conn
        .prepare("SELECT version FROM schema_migrations ORDER BY version ASC")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(versions, vec![1, 2, 3, 4, 5, 6]);

    // Verify SqliteStateRepository opens cleanly
    let repo = SqliteStateRepository::new(&db_path);
    assert!(repo.is_ok());
}

#[test]
fn test_incremental_migration_0001_to_0003_preserves_legacy_data() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("legacy.sqlite3");

    let conn = Connection::open(&db_path).unwrap();

    // 1. Manually apply 0001
    let mig_0001 = include_str!("../migrations/0001_initial.sql");
    conn.execute_batch(&format!(
        "BEGIN;\nCREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));\nCOMMIT;",
        mig_0001
    ))
    .unwrap();

    // Insert legacy data into 0001 schema
    conn.execute(
        "INSERT INTO game_installations (id, canonical_root, platform_kind, detected_version, validated_at, is_fresh)
         VALUES ('game-1', '/path/to/game', 'steam_native', '1.6.8', '2026-01-01T00:00:00Z', 1)",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO setups (id, game_id, display_name, relative_mods_dir, created_at)
         VALUES ('setup-1', 'game-1', 'Default Setup', 'Mods', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO packages (hash, original_filename, source_kind, byte_size, created_at)
         VALUES ('hash-abc', 'mod.zip', 'local_zip', 12345, '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO installed_mods (id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at)
         VALUES ('mod-1', 'setup-1', 'hash-abc', 'Author.Mod', 'My Mod', 'Author', '1.0.0', 'Test desc', '{}', 'Author.Mod', '[\"manifest.json\"]', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    // 2. Run migration runner (will apply 0002 and 0003)
    run_migrations(&conn).expect("Incremental migrations should succeed");

    // 3. Verify schema migrations table
    let versions: Vec<u32> = conn
        .prepare("SELECT version FROM schema_migrations ORDER BY version ASC")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(versions, vec![1, 2, 3, 4, 5, 6]);

    let game_uuid = manager_core::ids::derive_uuid("game-1").to_string();
    let profile_uuid = manager_core::ids::derive_uuid("setup-1").to_string();

    // 4. Verify setups migrated to profiles and legacy ids became UUIDs
    let (profile_id, profile_name, profile_game_id): (String, String, String) = conn
        .query_row(
            "SELECT id, name, game_installation_id FROM profiles WHERE id = ?1",
            [&profile_uuid],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(profile_id, profile_uuid);
    assert_eq!(profile_name, "Default Setup");
    assert_eq!(profile_game_id, game_uuid);

    // 5. Verify game profile context was initialized
    let (active_prof, default_prof): (String, String) = conn
        .query_row(
            "SELECT active_profile_id, default_profile_id FROM game_profile_context WHERE game_installation_id = ?1",
            [&game_uuid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(active_prof, profile_uuid);
    assert_eq!(default_prof, profile_uuid);

    // 6. Verify installed_mods migrated to profile_deployments & profile_components
    let depl_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM profile_deployments WHERE profile_id = ?1",
            [&profile_uuid],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(depl_count, 1);

    let comp_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM profile_components pc JOIN package_components pkg ON pc.package_component_id = pkg.id WHERE pc.profile_id = ?1 AND pkg.unique_id = 'Author.Mod'",
            [&profile_uuid],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(comp_count, 1);

    // 7. Verify package_components and acquisitions were created
    let pkg_comp_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM package_components WHERE artifact_hash = 'hash-abc' AND unique_id = 'Author.Mod'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pkg_comp_count, 1);

    let acq_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM acquisitions WHERE artifact_hash = 'hash-abc'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(acq_count, 1);
}
