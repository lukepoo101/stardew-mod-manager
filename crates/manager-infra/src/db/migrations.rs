use rusqlite::Connection;

pub const MIGRATION_0001: &str = include_str!("../../migrations/0001_initial.sql");
pub const MIGRATION_0002: &str = include_str!("../../migrations/0002_launch_log_baseline.sql");
pub const MIGRATION_0003: &str = include_str!("../../migrations/0003_architecture_foundation.sql");

pub fn run_migrations(conn: &Connection) -> Result<(), String> {
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

    Ok(())
}
