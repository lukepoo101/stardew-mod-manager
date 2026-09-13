use manager_core::ids::derive_uuid;
use rusqlite::Connection;
use std::path::Path;
use uuid::Uuid;

pub const MIGRATION_0001: &str = include_str!("../../migrations/0001_initial.sql");
pub const MIGRATION_0002: &str = include_str!("../../migrations/0002_launch_log_baseline.sql");
pub const MIGRATION_0003: &str = include_str!("../../migrations/0003_architecture_foundation.sql");
pub const MIGRATION_0004: &str = include_str!("../../migrations/0004_preferences.sql");

/// Runs migrations against a database with no profile storage beside it
/// (in-memory databases and tests).
pub fn run_migrations(conn: &Connection) -> Result<(), String> {
    run_migrations_with_storage(conn, None)
}

/// Runs migrations for a database whose profile directories live under
/// `data_dir/setups/<profile id>`.
pub fn run_migrations_with_storage(
    conn: &Connection,
    data_dir: Option<&Path>,
) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
        [],
    )
    .map_err(|e| format!("Failed to create schema_migrations table: {}", e))?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations;",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version < 1 {
        conn.execute_batch(&format!(
            "BEGIN;\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));\nCOMMIT;",
            MIGRATION_0001
        ))
        .map_err(|e| format!("Migration 0001 failed: {}", e))?;
    }

    if current_version < 2 {
        conn.execute_batch(&format!(
            "BEGIN;\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));\nCOMMIT;",
            MIGRATION_0002
        ))
        .map_err(|e| format!("Migration 0002 failed: {}", e))?;
    }

    if current_version < 3 {
        conn.execute_batch(&format!(
            "BEGIN;\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (3, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));\nCOMMIT;",
            MIGRATION_0003
        ))
        .map_err(|e| format!("Migration 0003 failed: {}", e))?;
    }

    if current_version < 4 {
        conn.execute_batch(&format!(
            "BEGIN;\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));\nCOMMIT;",
            MIGRATION_0004
        ))
        .map_err(|e| format!("Migration 0004 failed: {}", e))?;
    }

    if current_version < 5 {
        migrate_legacy_ids(conn, data_dir).map_err(|e| format!("Migration 0005 failed: {}", e))?;
    }

    Ok(())
}

/// Tables whose primary key was a prefixed string before the UUID identity
/// model, paired with the columns referencing them.
const LEGACY_ID_TABLES: &[(&str, &[(&str, &str)])] = &[
    (
        "game_installations",
        &[
            ("setups", "game_id"),
            ("smapi_installations", "game_id"),
            ("launch_sessions", "game_id"),
            ("profiles", "game_installation_id"),
            ("app_context", "active_game_installation_id"),
            ("game_profile_context", "game_installation_id"),
            ("operations", "game_installation_id"),
        ],
    ),
    (
        "setups",
        &[
            ("installed_mods", "setup_id"),
            ("launch_sessions", "setup_id"),
        ],
    ),
    (
        "profiles",
        &[
            ("profile_deployments", "profile_id"),
            ("profile_components", "profile_id"),
            ("game_profile_context", "active_profile_id"),
            ("game_profile_context", "default_profile_id"),
            ("game_profile_context", "last_active_profile_id"),
            ("operations", "profile_id"),
            ("operation_effects", "profile_id"),
            ("findings", "profile_id"),
        ],
    ),
    (
        "profile_deployments",
        &[("profile_components", "deployment_id")],
    ),
    (
        "package_components",
        &[("profile_components", "package_component_id")],
    ),
    (
        "operations",
        &[
            ("operation_resources", "operation_id"),
            ("operation_steps", "operation_id"),
            ("operation_effects", "operation_id"),
        ],
    ),
    ("installed_mods", &[]),
    ("profile_components", &[]),
    ("acquisitions", &[]),
    ("operation_effects", &[]),
    ("findings", &[]),
    ("launch_sessions", &[]),
    ("smapi_installations", &[]),
];

/// Columns holding loose entity identifiers rather than a typed foreign key.
const LEGACY_ID_VALUE_COLUMNS: &[(&str, &str)] = &[
    ("operation_resources", "resource_id"),
    ("operation_effects", "entity_id"),
];

/// Rewrites pre-UUID identifiers (`game-…`, `setup-…`, `op-…`, …) to stable
/// UUIDs so rows written by earlier versions stay readable.
///
/// Profile storage is addressed by profile id (`<data_dir>/setups/<id>/Mods`),
/// so the identifier rewrite has to move the directories with it. The mapping
/// is journalled and committed before any directory is touched, and every step
/// is idempotent, so an interrupted migration is completed by the next run
/// rather than leaving state stranded under an identifier nothing references.
fn migrate_legacy_ids(conn: &Connection, data_dir: Option<&Path>) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS legacy_id_migrations (
            legacy_id TEXT PRIMARY KEY,
            new_id TEXT NOT NULL,
            storage_migrated INTEGER NOT NULL DEFAULT 0,
            recorded_at TEXT NOT NULL
        );",
        [],
    )
    .map_err(|e| e.to_string())?;

    journal_legacy_ids(conn).map_err(|e| e.to_string())?;
    migrate_profile_storage(conn, data_dir)?;
    rewrite_legacy_ids(conn).map_err(|e| e.to_string())
}

/// Records the legacy -> UUID mapping for every profile so the directory move
/// can be resumed independently of the identifier rewrite.
fn journal_legacy_ids(conn: &Connection) -> rusqlite::Result<()> {
    for legacy in legacy_values(conn, "profiles", "id")? {
        conn.execute(
            "INSERT OR IGNORE INTO legacy_id_migrations (legacy_id, new_id, storage_migrated, recorded_at)
             VALUES (?1, ?2, 0, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
            (&legacy, derive_uuid(&legacy).to_string()),
        )?;
    }
    Ok(())
}

fn migrate_profile_storage(conn: &Connection, data_dir: Option<&Path>) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT legacy_id, new_id FROM legacy_id_migrations WHERE storage_migrated = 0")
        .map_err(|e| e.to_string())?;
    let pending = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    for (legacy_id, new_id) in pending {
        if let Some(data_dir) = data_dir {
            let from = data_dir.join("setups").join(&legacy_id);
            let to = data_dir.join("setups").join(&new_id);
            if from.exists() && !to.exists() {
                std::fs::rename(&from, &to).map_err(|e| {
                    format!(
                        "Failed to move profile storage from '{}' to '{}': {}",
                        from.display(),
                        to.display(),
                        e
                    )
                })?;
            } else if from.exists() {
                return Err(format!(
                    "Cannot move profile storage: both '{}' and '{}' exist",
                    from.display(),
                    to.display()
                ));
            }
        }

        conn.execute(
            "UPDATE legacy_id_migrations SET storage_migrated = 1 WHERE legacy_id = ?1",
            [&legacy_id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn rewrite_legacy_ids(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN; PRAGMA defer_foreign_keys = ON;")?;

    let result = (|| -> rusqlite::Result<()> {
        for (table, references) in LEGACY_ID_TABLES {
            for legacy in legacy_values(conn, table, "id")? {
                let replacement = derive_uuid(&legacy).to_string();
                for (ref_table, ref_column) in *references {
                    conn.execute(
                        &format!(
                            "UPDATE {ref_table} SET {ref_column} = ?1 WHERE {ref_column} = ?2"
                        ),
                        (&replacement, &legacy),
                    )?;
                }
                conn.execute(
                    &format!("UPDATE {table} SET id = ?1 WHERE id = ?2"),
                    (&replacement, &legacy),
                )?;
            }
        }

        for (table, column) in LEGACY_ID_VALUE_COLUMNS {
            for legacy in legacy_values(conn, table, column)? {
                conn.execute(
                    &format!("UPDATE {table} SET {column} = ?1 WHERE {column} = ?2"),
                    (derive_uuid(&legacy).to_string(), &legacy),
                )?;
            }
        }

        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (5, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
            [],
        )?;
        Ok(())
    })();

    match result {
        Ok(()) => conn.execute_batch("COMMIT;"),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(e)
        }
    }
}

fn legacy_values(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT DISTINCT {column} FROM {table} WHERE {column} IS NOT NULL"
    ))?;
    let values = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(values
        .into_iter()
        .filter(|value| Uuid::parse_str(value).is_err())
        .collect())
}
