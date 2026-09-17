use manager_core::ids::derive_uuid;
use manager_core::package::PackageComponent;
use manager_core::{ArtifactHash, ModUniqueId};
use rusqlite::params;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use uuid::Uuid;

pub const MIGRATION_0001: &str = include_str!("../../migrations/0001_initial.sql");
pub const MIGRATION_0002: &str = include_str!("../../migrations/0002_launch_log_baseline.sql");
pub const MIGRATION_0003: &str = include_str!("../../migrations/0003_architecture_foundation.sql");
pub const MIGRATION_0004: &str = include_str!("../../migrations/0004_preferences.sql");
pub const MIGRATION_0006: &str =
    include_str!("../../migrations/0006_package_component_identity.sql");
pub const MIGRATION_0007: &str =
    include_str!("../../migrations/0007_legacy_operation_reconciliation.sql");

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

    if current_version < 6 {
        migrate_package_component_ids(conn)
            .map_err(|e| format!("Migration 0006 identity rewrite failed: {}", e))?;
        conn.execute_batch(&format!(
            "BEGIN;\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (6, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));\nCOMMIT;",
            MIGRATION_0006
        ))
        .map_err(|e| format!("Migration 0006 failed: {}", e))?;
    }

    if current_version < 7 {
        migrate_legacy_operations(conn, data_dir).map_err(|e| {
            format!(
                "Migration 0007 legacy operation reconciliation failed: {}",
                e
            )
        })?;
        conn.execute_batch(&format!(
            "BEGIN;\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (7, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));\nCOMMIT;",
            MIGRATION_0007
        ))
        .map_err(|e| format!("Migration 0007 failed: {}", e))?;
    }

    Ok(())
}

/// Converts operation journals written by the pre-composition runtime into
/// modern recovery journals.  The old runtime used `pending`, `running`, and
/// `recovering`; leaving those strings in the modern table causes the decoder
/// to turn them into terminal `Failed` operations.  Every nonterminal legacy
/// row is therefore made explicitly recoverable before the old runtime is no
/// longer available to interpret it.
fn migrate_legacy_operations(conn: &Connection, data_dir: Option<&Path>) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, state, plan_json, error_json
             FROM operations
             WHERE profile_id IS NULL
               AND game_installation_id IS NULL
               AND state IN ('pending', 'prepared', 'running', 'recovering', 'recovery_required', 'completed', 'failed')
             ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    for (operation_id, kind, legacy_state, original_plan, original_error) in rows {
        let mut plan = serde_json::from_str::<Value>(&original_plan).unwrap_or(Value::Null);
        let mut profile_id = None;
        let mut game_installation_id = None;
        let mut reconciliation_error = None;

        match kind.as_str() {
            "mod_install" | "mod_remove" => {
                let setup_id = plan
                    .get("setup_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(setup_id) = setup_id {
                    match resolve_migrated_id(conn, "profiles", &setup_id)? {
                        Some(resolved_profile_id) => {
                            game_installation_id = conn
                                .query_row(
                                    "SELECT game_installation_id FROM profiles WHERE id = ?1",
                                    [&resolved_profile_id],
                                    |row| row.get::<_, String>(0),
                                )
                                .optional()
                                .map_err(|e| e.to_string())?;
                            profile_id = Some(resolved_profile_id.clone());
                            plan["setup_id"] = json!(resolved_profile_id);
                            plan["profile_id"] = json!(resolved_profile_id);
                        }
                        None => {
                            reconciliation_error = Some(format!(
                                "Legacy operation '{}' references unknown setup '{}'",
                                operation_id, setup_id
                            ));
                        }
                    }
                } else {
                    reconciliation_error = Some(format!(
                        "Legacy operation '{}' has no setup_id in its recovery plan",
                        operation_id
                    ));
                }

                if kind == "mod_remove" {
                    normalize_legacy_removal_plan(
                        conn,
                        data_dir,
                        &operation_id,
                        &mut plan,
                        profile_id.as_deref(),
                    )?;
                }
            }
            "smapi_setup" => {
                let game_id = plan
                    .get("game_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(game_id) = game_id {
                    game_installation_id =
                        resolve_migrated_id(conn, "game_installations", &game_id)?;
                    if let Some(ref resolved) = game_installation_id {
                        plan["game_id"] = json!(resolved);
                    } else {
                        reconciliation_error = Some(format!(
                            "Legacy SMAPI operation '{}' references unknown game '{}'",
                            operation_id, game_id
                        ));
                    }
                } else {
                    reconciliation_error = Some(format!(
                        "Legacy SMAPI operation '{}' has no game_id in its recovery plan",
                        operation_id
                    ));
                }
            }
            _ => {
                reconciliation_error = Some(format!(
                    "Legacy operation '{}' has unsupported kind '{}'",
                    operation_id, kind
                ));
            }
        }

        let plan_json = serde_json::to_string(&plan).map_err(|e| e.to_string())?;
        let error_json = original_error.or_else(|| {
            reconciliation_error.as_ref().map(|message| {
                json!({
                    "code": "LEGACY_OPERATION_REQUIRES_RECONCILIATION",
                    "message": message,
                    "recoverable": true,
                })
                .to_string()
            })
        });
        let target_state = match legacy_state.as_str() {
            "completed" => "succeeded",
            "failed" => "failed",
            _ => "recovery_required",
        };
        conn.execute(
            "UPDATE operations
             SET state = ?1,
                 plan_json = ?2,
                 error_json = ?3,
                 error_code = COALESCE(error_code, 'LEGACY_OPERATION_REQUIRES_RECONCILIATION'),
                 game_installation_id = ?4,
                 profile_id = ?5,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE id = ?6",
            params![
                target_state,
                plan_json,
                error_json,
                game_installation_id,
                profile_id,
                operation_id,
            ],
        )
        .map_err(|e| e.to_string())?;

        if let Some(profile_id) = profile_id {
            conn.execute(
                "INSERT OR IGNORE INTO operation_resources (operation_id, resource_kind, resource_id, access_mode)
                 VALUES (?1, 'profile', ?2, 'write')",
                params![operation_id, profile_id],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

fn resolve_migrated_id(
    conn: &Connection,
    table: &str,
    value: &str,
) -> Result<Option<String>, String> {
    let direct = conn
        .query_row(
            &format!("SELECT id FROM {table} WHERE id = ?1"),
            [value],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if direct.is_some() {
        return Ok(direct);
    }

    let derived = derive_uuid(value).to_string();
    let migrated = conn
        .query_row(
            &format!("SELECT id FROM {table} WHERE id = ?1"),
            [&derived],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(migrated)
}

fn normalize_legacy_removal_plan(
    conn: &Connection,
    data_dir: Option<&Path>,
    operation_id: &str,
    plan: &mut Value,
    profile_id: Option<&str>,
) -> Result<(), String> {
    let Some(profile_id) = profile_id else {
        return Ok(());
    };

    if let Some(installed_mod_id) = plan
        .get("installed_mod_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
    {
        if let Some(resolved) = resolve_migrated_id(conn, "installed_mods", &installed_mod_id)? {
            plan["installed_mod_id"] = json!(resolved);
        }
    }
    if let Some(bundle_mod_ids) = plan.get("bundle_mod_ids").and_then(Value::as_array) {
        let resolved = bundle_mod_ids
            .iter()
            .filter_map(Value::as_str)
            .map(|id| resolve_migrated_id(conn, "installed_mods", id))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|id| id.unwrap_or_default())
            .filter(|id| !id.is_empty())
            .map(Value::String)
            .collect::<Vec<_>>();
        if !resolved.is_empty() {
            plan["bundle_mod_ids"] = Value::Array(resolved);
        }
    }

    let relative_path = plan
        .get("relative_folder_path")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let legacy_operation_id = plan
        .get("operation_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let (Some(relative_path), Some(legacy_operation_id)) = (relative_path, legacy_operation_id) {
        plan["deployment_rel_path"] = json!(relative_path);
        plan["operation_id"] = json!(operation_id);
        if let Some(data_dir) = data_dir {
            if manager_core::install::validate_relative_path(&relative_path).is_ok()
                && is_safe_storage_component(&legacy_operation_id)
            {
                let profile_root = data_dir.join("setups").join(profile_id);
                let legacy_root = profile_root.join(".recovery").join(&legacy_operation_id);
                let current_root = profile_root.join(".recovery").join(operation_id);
                if legacy_root != current_root && legacy_root.exists() {
                    if current_root.exists() {
                        return Err(format!(
                            "Cannot reconcile legacy recovery directory '{}' because '{}' already exists",
                            legacy_root.display(),
                            current_root.display()
                        ));
                    }
                    std::fs::create_dir_all(
                        current_root
                            .parent()
                            .ok_or_else(|| "Recovery directory has no parent".to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    crate::platform::shared::fs::rename_path(&legacy_root, &current_root).map_err(
                        |e| {
                            format!(
                                "Failed to move legacy recovery directory '{}' to '{}': {}",
                                legacy_root.display(),
                                current_root.display(),
                                e
                            )
                        },
                    )?;
                }
                plan["recovery_folder_path"] = json!(current_root
                    .join(&relative_path)
                    .to_string_lossy()
                    .to_string());
            }
        }
    }

    Ok(())
}

fn is_safe_storage_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
}

/// Rewrites pre-0006 component IDs to the same deterministic IDs used by new
/// installs. References are updated before duplicate rows are removed, and
/// deferred foreign keys keep the rewrite atomic from SQLite's perspective.
fn migrate_package_component_ids(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, artifact_hash, relative_component_root, unique_id
             FROM package_components ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    let mut groups: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
    for (id, hash, root, unique_id) in rows {
        groups.entry((hash, root, unique_id)).or_default().push(id);
    }

    conn.execute_batch("BEGIN; PRAGMA defer_foreign_keys = ON;")
        .map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        for ((hash, root, unique_id), ids) in groups {
            let canonical = PackageComponent::canonical_id(
                &ArtifactHash::new(hash),
                &root,
                &ModUniqueId::new(unique_id),
            )
            .to_string();
            let survivor = ids
                .first()
                .ok_or_else(|| "empty package component group".to_string())?;

            for old_id in &ids {
                conn.execute(
                    "UPDATE profile_components SET package_component_id = ?1 WHERE package_component_id = ?2",
                    params![canonical, old_id],
                )
                .map_err(|e| e.to_string())?;
            }
            for duplicate in ids.iter().skip(1) {
                conn.execute(
                    "DELETE FROM package_components WHERE id = ?1",
                    params![duplicate],
                )
                .map_err(|e| e.to_string())?;
            }
            conn.execute(
                "UPDATE package_components SET id = ?1 WHERE id = ?2",
                params![canonical, survivor],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();

    match result {
        Ok(()) => conn.execute_batch("COMMIT;").map_err(|e| e.to_string()),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(e)
        }
    }
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
                crate::platform::shared::fs::rename_path(&from, &to).map_err(|e| {
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
