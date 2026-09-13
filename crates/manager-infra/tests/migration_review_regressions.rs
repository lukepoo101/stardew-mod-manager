use manager_infra::db::migrations::run_migrations;
use rusqlite::Connection;

fn apply_v1(conn: &Connection) {
    conn.execute_batch(&format!(
        "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (1, '2026-01-01T00:00:00Z');",
        include_str!("../migrations/0001_initial.sql")
    ))
    .unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
}

fn insert_game_and_setup(conn: &Connection) {
    conn.execute(
        "INSERT INTO game_installations (id, canonical_root, platform_kind, detected_version, validated_at, is_fresh)
         VALUES ('game-legacy', '/games/Stardew Valley', 'steam_native', '1.6.8', '2026-01-01T00:00:00Z', 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO setups (id, game_id, display_name, relative_mods_dir, created_at)
         VALUES ('setup-legacy', 'game-legacy', 'Default', 'Mods', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
}

fn manifest(unique_id: &str, name: &str) -> String {
    format!(
        r#"{{"Name":"{name}","Author":"Author","Version":"1.0.0","UniqueID":"{unique_id}","EntryDll":"Mod.dll"}}"#
    )
}

#[test]
fn legacy_installed_mod_without_package_row_gets_a_placeholder_artifact() {
    let conn = Connection::open_in_memory().unwrap();
    apply_v1(&conn);
    insert_game_and_setup(&conn);

    let missing_hash = "a".repeat(64);
    conn.execute(
        "INSERT INTO installed_mods (id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at)
         VALUES (?1, 'setup-legacy', ?2, 'Author.Mod', 'Legacy Mod', 'Author', '1.0.0', NULL, ?3, 'LegacyMod', '[\"manifest.json\"]', '2026-01-01T00:00:00Z')",
        rusqlite::params!["mod-legacy", missing_hash, manifest("Author.Mod", "Legacy Mod")],
    )
    .unwrap();

    run_migrations(&conn).expect("legacy rows without retained package metadata must migrate");

    let (source_kind, byte_size, storage_path): (String, i64, String) = conn
        .query_row(
            "SELECT source_kind, byte_size, storage_relative_path FROM packages WHERE hash = ?1",
            [&missing_hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(source_kind, "legacy_missing");
    assert_eq!(byte_size, 0);
    assert_eq!(storage_path, format!("packages/{missing_hash}.zip"));

    let acquisition_source: String = conn
        .query_row(
            "SELECT source FROM acquisitions WHERE artifact_hash = ?1",
            [&missing_hash],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(acquisition_source, "manual_reference");

    let deployment_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM profile_deployments WHERE artifact_hash = ?1",
            [&missing_hash],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(deployment_count, 1);
}

#[test]
fn legacy_bundle_rows_share_one_profile_deployment() {
    let conn = Connection::open_in_memory().unwrap();
    apply_v1(&conn);
    insert_game_and_setup(&conn);

    let package_hash = "b".repeat(64);
    conn.execute(
        "INSERT INTO packages (hash, original_filename, source_kind, byte_size, created_at)
         VALUES (?1, 'bundle.zip', 'local_zip', 1234, '2026-01-01T00:00:00Z')",
        [&package_hash],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO installed_mods (id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at)
         VALUES ('mod-main', 'setup-legacy', ?1, 'Author.Main', 'Main Mod', 'Author', '1.0.0', NULL, ?2, 'BundleRoot', '[\"manifest.json\"]', '2026-01-01T00:00:00Z')",
        rusqlite::params![package_hash, manifest("Author.Main", "Main Mod")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO installed_mods (id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at)
         VALUES ('mod-companion', 'setup-legacy', ?1, 'Author.Companion', 'Companion', 'Author', '1.0.0', NULL, ?2, 'BundleRoot/Companion', '[\"Companion/manifest.json\"]', '2026-01-01T00:00:00Z')",
        rusqlite::params![package_hash, manifest("Author.Companion", "Companion")],
    )
    .unwrap();

    run_migrations(&conn).expect("legacy bundle must migrate");

    let profile_id = manager_core::ids::derive_uuid("setup-legacy").to_string();
    let (deployment_count, root): (i64, String) = conn
        .query_row(
            "SELECT COUNT(*), MIN(root_relative_path) FROM profile_deployments WHERE profile_id = ?1",
            [&profile_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(deployment_count, 1);
    assert_eq!(root, "BundleRoot");

    let (component_count, distinct_deployments): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COUNT(DISTINCT deployment_id) FROM profile_components WHERE profile_id = ?1",
            [&profile_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(component_count, 2);
    assert_eq!(distinct_deployments, 1);

    let roots: Vec<String> = conn
        .prepare(
            "SELECT relative_component_root FROM package_components WHERE artifact_hash = ?1 ORDER BY relative_component_root",
        )
        .unwrap()
        .query_map([&package_hash], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(roots, vec!["".to_string(), "Companion".to_string()]);

    let reasons: Vec<String> = conn
        .prepare(
            "SELECT installed_reason FROM profile_components WHERE profile_id = ?1 ORDER BY installed_reason",
        )
        .unwrap()
        .query_map([&profile_id], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        reasons,
        vec!["bundle_companion".to_string(), "direct".to_string()]
    );
}
