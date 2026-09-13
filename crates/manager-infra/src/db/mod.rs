use chrono::{DateTime, Utc};
use manager_core::domain::*;
use manager_core::ports::StateRepository;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct SqliteStateRepository {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStateRepository {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, String> {
        let path = db_path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create db parent directory: {}", e))?;
        }

        let conn = Connection::open(path).map_err(|e| {
            format!(
                "Failed to open SQLite database at '{}': {}",
                path.display(),
                e
            )
        })?;

        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| format!("Failed to enable foreign keys: {}", e))?;

        let repo = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        repo.run_migrations()?;
        Ok(repo)
    }

    pub fn new_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("Failed to open in-memory SQLite db: {}", e))?;

        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| format!("Failed to enable foreign keys: {}", e))?;

        let repo = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        repo.run_migrations()?;
        Ok(repo)
    }

    fn run_migrations(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );",
            [],
        )
        .map_err(|e| format!("Failed to create migrations table: {}", e))?;

        let current_version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations;",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if current_version < 1 {
            conn.execute_batch(
                "BEGIN;
                CREATE TABLE game_installations (
                    id TEXT PRIMARY KEY,
                    canonical_root TEXT NOT NULL,
                    platform_kind TEXT NOT NULL,
                    detected_version TEXT,
                    validated_at TEXT NOT NULL,
                    is_fresh INTEGER NOT NULL,
                    validation_error TEXT
                );

                CREATE TABLE setups (
                    id TEXT PRIMARY KEY,
                    game_id TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    relative_mods_dir TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY (game_id) REFERENCES game_installations(id) ON DELETE CASCADE
                );

                CREATE TABLE packages (
                    hash TEXT PRIMARY KEY,
                    original_filename TEXT NOT NULL,
                    source_kind TEXT NOT NULL,
                    byte_size INTEGER NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE TABLE installed_mods (
                    id TEXT PRIMARY KEY,
                    setup_id TEXT NOT NULL,
                    package_id TEXT NOT NULL,
                    unique_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    author TEXT NOT NULL,
                    version TEXT NOT NULL,
                    description TEXT,
                    raw_manifest TEXT NOT NULL,
                    relative_target_path TEXT NOT NULL,
                    file_inventory_json TEXT NOT NULL,
                    installed_at TEXT NOT NULL,
                    FOREIGN KEY (setup_id) REFERENCES setups(id) ON DELETE CASCADE
                );

                CREATE TABLE operations (
                    id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL,
                    state TEXT NOT NULL,
                    plan_json TEXT NOT NULL,
                    error_json TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    schema_version INTEGER NOT NULL
                );

                CREATE TABLE launch_sessions (
                    id TEXT PRIMARY KEY,
                    game_id TEXT NOT NULL,
                    setup_id TEXT NOT NULL,
                    launched_at TEXT NOT NULL,
                    pid INTEGER,
                    state TEXT NOT NULL,
                    expected_mod_ids_json TEXT NOT NULL,
                    log_baseline_time TEXT,
                    verification_result_json TEXT
                );

                CREATE TABLE smapi_installations (
                    id TEXT PRIMARY KEY,
                    game_id TEXT NOT NULL UNIQUE,
                    release_version TEXT NOT NULL,
                    adapter_version TEXT NOT NULL,
                    observed_version TEXT,
                    installed_at TEXT NOT NULL,
                    FOREIGN KEY (game_id) REFERENCES game_installations(id) ON DELETE CASCADE
                );

                CREATE TABLE window_geometry (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    schema_version INTEGER NOT NULL,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL,
                    x INTEGER NOT NULL,
                    y INTEGER NOT NULL,
                    is_maximized INTEGER NOT NULL
                );

                INSERT INTO schema_migrations (version, applied_at) VALUES (1, datetime('now'));
                COMMIT;",
            )
            .map_err(|e| format!("Migration 1 failed: {}", e))?;
        }

        if current_version < 2 {
            conn.execute_batch("BEGIN; ALTER TABLE launch_sessions ADD COLUMN log_baseline_json TEXT;
                INSERT INTO schema_migrations(version, applied_at) VALUES(2, datetime('now')); COMMIT;")
                .map_err(|e| format!("Migration 2 failed: {e}"))?;
        }

        Ok(())
    }
}

impl StateRepository for SqliteStateRepository {
    fn save_game(&self, game: &GameInstallation) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let kind_str = match game.platform_kind {
            StoreKind::SteamNative => "steam_native",
            StoreKind::ManualFolder => "manual_folder",
            StoreKind::Unsupported => "unsupported",
        };

        conn.execute(
            "INSERT INTO game_installations (id, canonical_root, platform_kind, detected_version, validated_at, is_fresh, validation_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(id) DO UPDATE SET canonical_root=excluded.canonical_root, platform_kind=excluded.platform_kind, detected_version=excluded.detected_version, validated_at=excluded.validated_at, is_fresh=excluded.is_fresh, validation_error=excluded.validation_error",
            params![
                game.id,
                game.canonical_root.to_string_lossy().to_string(),
                kind_str,
                game.detected_version,
                game.validated_at.to_rfc3339(),
                if game.is_fresh { 1 } else { 0 },
                game.validation_error,
            ],
        )
        .map_err(|e| format!("Failed to save game: {}", e))?;
        Ok(())
    }

    fn get_game(&self, id: &str) -> Result<Option<GameInstallation>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, canonical_root, platform_kind, detected_version, validated_at, is_fresh, validation_error FROM game_installations WHERE id = ?")
            .map_err(|e| e.to_string())?;

        let game = stmt
            .query_row(params![id], |row| {
                let kind_str: String = row.get(2)?;
                let platform_kind = match kind_str.as_str() {
                    "steam_native" => StoreKind::SteamNative,
                    "manual_folder" => StoreKind::ManualFolder,
                    _ => StoreKind::Unsupported,
                };
                let val_time_str: String = row.get(4)?;
                let validated_at = DateTime::parse_from_rfc3339(&val_time_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let is_fresh_int: i64 = row.get(5)?;

                Ok(GameInstallation {
                    id: row.get(0)?,
                    canonical_root: PathBuf::from(row.get::<_, String>(1)?),
                    platform_kind,
                    detected_version: row.get(3)?,
                    validated_at,
                    is_fresh: is_fresh_int != 0,
                    is_managed: true,
                    validation_error: row.get(6)?,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(game)
    }

    fn list_games(&self) -> Result<Vec<GameInstallation>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, canonical_root, platform_kind, detected_version, validated_at, is_fresh, validation_error FROM game_installations")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let kind_str: String = row.get(2)?;
                let platform_kind = match kind_str.as_str() {
                    "steam_native" => StoreKind::SteamNative,
                    "manual_folder" => StoreKind::ManualFolder,
                    _ => StoreKind::Unsupported,
                };
                let val_time_str: String = row.get(4)?;
                let validated_at = DateTime::parse_from_rfc3339(&val_time_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let is_fresh_int: i64 = row.get(5)?;

                Ok(GameInstallation {
                    id: row.get(0)?,
                    canonical_root: PathBuf::from(row.get::<_, String>(1)?),
                    platform_kind,
                    detected_version: row.get(3)?,
                    validated_at,
                    is_fresh: is_fresh_int != 0,
                    is_managed: true,
                    validation_error: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(|e| e.to_string())?);
        }
        Ok(list)
    }

    fn save_setup(&self, setup: &Setup) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO setups (id, game_id, display_name, relative_mods_dir, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                setup.id,
                setup.game_id,
                setup.display_name,
                setup.relative_mods_dir,
                setup.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| format!("Failed to save setup: {}", e))?;
        Ok(())
    }

    fn get_setup(&self, id: &str) -> Result<Option<Setup>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, game_id, display_name, relative_mods_dir, created_at FROM setups WHERE id = ?")
            .map_err(|e| e.to_string())?;

        let setup = stmt
            .query_row(params![id], |row| {
                let time_str: String = row.get(4)?;
                let created_at = DateTime::parse_from_rfc3339(&time_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(Setup {
                    id: row.get(0)?,
                    game_id: row.get(1)?,
                    display_name: row.get(2)?,
                    relative_mods_dir: row.get(3)?,
                    created_at,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(setup)
    }

    fn get_default_setup(&self, game_id: &str) -> Result<Option<Setup>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, game_id, display_name, relative_mods_dir, created_at FROM setups WHERE game_id = ? ORDER BY created_at ASC LIMIT 1")
            .map_err(|e| e.to_string())?;

        let setup = stmt
            .query_row(params![game_id], |row| {
                let time_str: String = row.get(4)?;
                let created_at = DateTime::parse_from_rfc3339(&time_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(Setup {
                    id: row.get(0)?,
                    game_id: row.get(1)?,
                    display_name: row.get(2)?,
                    relative_mods_dir: row.get(3)?,
                    created_at,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(setup)
    }

    fn save_package(&self, pkg: &Package) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO packages (hash, original_filename, source_kind, byte_size, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                pkg.hash,
                pkg.original_filename,
                pkg.source_kind,
                pkg.byte_size as i64,
                pkg.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| format!("Failed to save package: {}", e))?;
        Ok(())
    }

    fn get_package(&self, hash: &str) -> Result<Option<Package>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT hash, original_filename, source_kind, byte_size, created_at FROM packages WHERE hash = ?")
            .map_err(|e| e.to_string())?;

        let pkg = stmt
            .query_row(params![hash], |row| {
                let time_str: String = row.get(4)?;
                let created_at = DateTime::parse_from_rfc3339(&time_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(Package {
                    hash: row.get(0)?,
                    original_filename: row.get(1)?,
                    source_kind: row.get(2)?,
                    byte_size: row.get::<_, i64>(3)? as u64,
                    created_at,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(pkg)
    }

    fn save_installed_mod(&self, m: &InstalledMod) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let inv_json = serde_json::to_string(&m.file_inventory).unwrap_or_default();
        conn.execute(
            "INSERT OR REPLACE INTO installed_mods (id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                m.id,
                m.setup_id,
                m.package_id,
                m.unique_id,
                m.name,
                m.author,
                m.version,
                m.description,
                m.raw_manifest,
                m.relative_target_path,
                inv_json,
                m.installed_at.to_rfc3339(),
            ],
        )
        .map_err(|e| format!("Failed to save installed mod: {}", e))?;
        Ok(())
    }

    fn get_installed_mod(&self, id: &str) -> Result<Option<InstalledMod>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at FROM installed_mods WHERE id = ?")
            .map_err(|e| e.to_string())?;

        let item = stmt
            .query_row(params![id], |row| {
                let time_str: String = row.get(11)?;
                let installed_at = DateTime::parse_from_rfc3339(&time_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let inv_json: String = row.get(10)?;
                let file_inventory = serde_json::from_str(&inv_json).unwrap_or_default();

                Ok(InstalledMod {
                    id: row.get(0)?,
                    setup_id: row.get(1)?,
                    package_id: row.get(2)?,
                    unique_id: row.get(3)?,
                    name: row.get(4)?,
                    author: row.get(5)?,
                    version: row.get(6)?,
                    description: row.get(7)?,
                    raw_manifest: row.get(8)?,
                    relative_target_path: row.get(9)?,
                    file_inventory,
                    installed_at,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(item)
    }

    fn list_installed_mods(&self, setup_id: &str) -> Result<Vec<InstalledMod>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at FROM installed_mods WHERE setup_id = ? ORDER BY name ASC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![setup_id], |row| {
                let time_str: String = row.get(11)?;
                let installed_at = DateTime::parse_from_rfc3339(&time_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let inv_json: String = row.get(10)?;
                let file_inventory = serde_json::from_str(&inv_json).unwrap_or_default();

                Ok(InstalledMod {
                    id: row.get(0)?,
                    setup_id: row.get(1)?,
                    package_id: row.get(2)?,
                    unique_id: row.get(3)?,
                    name: row.get(4)?,
                    author: row.get(5)?,
                    version: row.get(6)?,
                    description: row.get(7)?,
                    raw_manifest: row.get(8)?,
                    relative_target_path: row.get(9)?,
                    file_inventory,
                    installed_at,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(|e| e.to_string())?);
        }
        Ok(list)
    }

    fn delete_installed_mod(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM installed_mods WHERE id = ?", params![id])
            .map_err(|e| format!("Failed to delete installed mod: {}", e))?;
        Ok(())
    }

    fn save_operation(&self, op: &Operation) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let kind_str = match op.kind {
            OperationKind::SmapiSetup => "smapi_setup",
            OperationKind::ModInstall => "mod_install",
            OperationKind::ModRemove => "mod_remove",
            OperationKind::GameLaunch => "game_launch",
        };
        let state_str = match op.state {
            OperationState::Pending => "pending",
            OperationState::Prepared => "prepared",
            OperationState::Running => "running",
            OperationState::Completed => "completed",
            OperationState::Failed => "failed",
            OperationState::Recovering => "recovering",
        };

        conn.execute(
            "INSERT OR REPLACE INTO operations (id, kind, state, plan_json, error_json, created_at, updated_at, schema_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                op.id,
                kind_str,
                state_str,
                op.plan_json,
                op.error_json,
                op.created_at.to_rfc3339(),
                op.updated_at.to_rfc3339(),
                op.schema_version,
            ],
        )
        .map_err(|e| format!("Failed to save operation: {}", e))?;
        Ok(())
    }

    fn get_operation(&self, id: &str) -> Result<Option<Operation>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, kind, state, plan_json, error_json, created_at, updated_at, schema_version FROM operations WHERE id = ?")
            .map_err(|e| e.to_string())?;

        let op = stmt
            .query_row(params![id], |row| {
                let kind_str: String = row.get(1)?;
                let kind = match kind_str.as_str() {
                    "smapi_setup" => OperationKind::SmapiSetup,
                    "mod_install" => OperationKind::ModInstall,
                    "mod_remove" => OperationKind::ModRemove,
                    _ => OperationKind::GameLaunch,
                };
                let state_str: String = row.get(2)?;
                let state = match state_str.as_str() {
                    "pending" => OperationState::Pending,
                    "prepared" => OperationState::Prepared,
                    "running" => OperationState::Running,
                    "completed" => OperationState::Completed,
                    "recovering" => OperationState::Recovering,
                    _ => OperationState::Failed,
                };

                let c_time: String = row.get(5)?;
                let u_time: String = row.get(6)?;

                Ok(Operation {
                    id: row.get(0)?,
                    kind,
                    state,
                    plan_json: row.get(3)?,
                    error_json: row.get(4)?,
                    created_at: DateTime::parse_from_rfc3339(&c_time)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    updated_at: DateTime::parse_from_rfc3339(&u_time)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    schema_version: row.get(7)?,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(op)
    }

    fn update_operation_state(
        &self,
        id: &str,
        state: OperationState,
        error_json: Option<String>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let state_str = match state {
            OperationState::Pending => "pending",
            OperationState::Prepared => "prepared",
            OperationState::Running => "running",
            OperationState::Completed => "completed",
            OperationState::Failed => "failed",
            OperationState::Recovering => "recovering",
        };

        conn.execute(
            "UPDATE operations SET state = ?1, error_json = ?2, updated_at = ?3 WHERE id = ?4",
            params![state_str, error_json, Utc::now().to_rfc3339(), id],
        )
        .map_err(|e| format!("Failed to update operation state: {}", e))?;
        Ok(())
    }

    fn list_unresolved_operations(&self) -> Result<Vec<Operation>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, kind, state, plan_json, error_json, created_at, updated_at, schema_version FROM operations WHERE state NOT IN ('completed', 'failed')")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let kind_str: String = row.get(1)?;
                let kind = match kind_str.as_str() {
                    "smapi_setup" => OperationKind::SmapiSetup,
                    "mod_install" => OperationKind::ModInstall,
                    "mod_remove" => OperationKind::ModRemove,
                    _ => OperationKind::GameLaunch,
                };
                let state_str: String = row.get(2)?;
                let state = match state_str.as_str() {
                    "pending" => OperationState::Pending,
                    "prepared" => OperationState::Prepared,
                    "running" => OperationState::Running,
                    "completed" => OperationState::Completed,
                    "recovering" => OperationState::Recovering,
                    _ => OperationState::Failed,
                };

                let c_time: String = row.get(5)?;
                let u_time: String = row.get(6)?;

                Ok(Operation {
                    id: row.get(0)?,
                    kind,
                    state,
                    plan_json: row.get(3)?,
                    error_json: row.get(4)?,
                    created_at: DateTime::parse_from_rfc3339(&c_time)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    updated_at: DateTime::parse_from_rfc3339(&u_time)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    schema_version: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(|e| e.to_string())?);
        }
        Ok(list)
    }

    fn save_launch_session(&self, session: &LaunchSession) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let state_str = match session.state {
            SessionState::Starting => "starting",
            SessionState::RunningUnverified => "running_unverified",
            SessionState::ModLoadConfirmed => "mod_load_confirmed",
            SessionState::Exited => "exited",
            SessionState::Failed => "failed",
            SessionState::VerificationUnavailable => "verification_unavailable",
        };

        let exp_json = serde_json::to_string(&session.expected_mod_ids).unwrap_or_default();
        let res_json = session
            .verification_result
            .as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_default());

        conn.execute(
            "INSERT OR REPLACE INTO launch_sessions (id, game_id, setup_id, launched_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                session.id,
                session.game_id,
                session.setup_id,
                session.launched_at.to_rfc3339(),
                session.pid.map(|p| p as i64),
                state_str,
                exp_json,
                session.log_baseline_time.map(|t| t.to_rfc3339()),
                res_json,
                session.log_baseline.as_ref().map(serde_json::to_string).transpose().map_err(|e| e.to_string())?,
            ],
        )
        .map_err(|e| format!("Failed to save launch session: {}", e))?;
        Ok(())
    }

    fn get_launch_session(&self, id: &str) -> Result<Option<LaunchSession>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, game_id, setup_id, launched_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json FROM launch_sessions WHERE id = ?")
            .map_err(|e| e.to_string())?;

        let sess = stmt
            .query_row(params![id], |row| {
                let state_str: String = row.get(5)?;
                let state = match state_str.as_str() {
                    "starting" => SessionState::Starting,
                    "running_unverified" => SessionState::RunningUnverified,
                    "mod_load_confirmed" => SessionState::ModLoadConfirmed,
                    "exited" => SessionState::Exited,
                    "verification_unavailable" => SessionState::VerificationUnavailable,
                    _ => SessionState::Failed,
                };
                let l_time: String = row.get(3)?;
                let pid_opt: Option<i64> = row.get(4)?;
                let exp_json: String = row.get(6)?;
                let exp_mods: Vec<String> = serde_json::from_str(&exp_json).unwrap_or_default();
                let b_time_opt: Option<String> = row.get(7)?;
                let log_baseline = b_time_opt.and_then(|t| {
                    DateTime::parse_from_rfc3339(&t)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                });
                let res_json_opt: Option<String> = row.get(8)?;
                let ver_res: Option<VerificationResult> =
                    res_json_opt.and_then(|j| serde_json::from_str(&j).ok());

                Ok(LaunchSession {
                    id: row.get(0)?,
                    game_id: row.get(1)?,
                    setup_id: row.get(2)?,
                    launched_at: DateTime::parse_from_rfc3339(&l_time)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    pid: pid_opt.map(|p| p as u32),
                    state,
                    expected_mod_ids: exp_mods,
                    log_baseline_time: log_baseline,
                    log_baseline: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|j| serde_json::from_str(&j).ok()),
                    verification_result: ver_res,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(sess)
    }

    fn get_latest_launch_session(
        &self,
        game_id: Option<&str>,
    ) -> Result<Option<LaunchSession>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let sql = match game_id {
            Some(_) => "SELECT id, game_id, setup_id, launched_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json FROM launch_sessions WHERE game_id = ? ORDER BY launched_at DESC LIMIT 1",
            None => "SELECT id, game_id, setup_id, launched_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json FROM launch_sessions ORDER BY launched_at DESC LIMIT 1",
        };
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

        let mapper = |row: &rusqlite::Row| {
            let state_str: String = row.get(5)?;
            let state = match state_str.as_str() {
                "starting" => SessionState::Starting,
                "running_unverified" => SessionState::RunningUnverified,
                "mod_load_confirmed" => SessionState::ModLoadConfirmed,
                "exited" => SessionState::Exited,
                "verification_unavailable" => SessionState::VerificationUnavailable,
                _ => SessionState::Failed,
            };
            let l_time: String = row.get(3)?;
            let pid_opt: Option<i64> = row.get(4)?;
            let exp_json: String = row.get(6)?;
            let exp_mods: Vec<String> = serde_json::from_str(&exp_json).unwrap_or_default();
            let b_time_opt: Option<String> = row.get(7)?;
            let log_baseline = b_time_opt.and_then(|t| {
                DateTime::parse_from_rfc3339(&t)
                    .ok()
                    .map(|d| d.with_timezone(&Utc))
            });
            let res_json_opt: Option<String> = row.get(8)?;
            let ver_res: Option<VerificationResult> =
                res_json_opt.and_then(|j| serde_json::from_str(&j).ok());

            Ok(LaunchSession {
                id: row.get(0)?,
                game_id: row.get(1)?,
                setup_id: row.get(2)?,
                launched_at: DateTime::parse_from_rfc3339(&l_time)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                pid: pid_opt.map(|p| p as u32),
                state,
                expected_mod_ids: exp_mods,
                log_baseline_time: log_baseline,
                log_baseline: row
                    .get::<_, Option<String>>(9)?
                    .and_then(|j| serde_json::from_str(&j).ok()),
                verification_result: ver_res,
            })
        };

        let sess = match game_id {
            Some(gid) => stmt
                .query_row(params![gid], mapper)
                .optional()
                .map_err(|e| e.to_string())?,
            None => stmt
                .query_row([], mapper)
                .optional()
                .map_err(|e| e.to_string())?,
        };

        Ok(sess)
    }

    fn update_launch_session(&self, session: &LaunchSession) -> Result<(), String> {
        self.save_launch_session(session)
    }

    fn save_smapi_installation(&self, rec: &SmapiInstallationRecord) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO smapi_installations (id, game_id, release_version, adapter_version, observed_version, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                rec.id,
                rec.game_id,
                rec.release_version,
                rec.adapter_version,
                rec.observed_version,
                rec.installed_at.to_rfc3339(),
            ],
        )
        .map_err(|e| format!("Failed to save SMAPI installation record: {}", e))?;
        Ok(())
    }

    fn get_smapi_installation(
        &self,
        game_id: &str,
    ) -> Result<Option<SmapiInstallationRecord>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, game_id, release_version, adapter_version, observed_version, installed_at FROM smapi_installations WHERE game_id = ?")
            .map_err(|e| e.to_string())?;

        let rec = stmt
            .query_row(params![game_id], |row| {
                let time_str: String = row.get(5)?;
                Ok(SmapiInstallationRecord {
                    id: row.get(0)?,
                    game_id: row.get(1)?,
                    release_version: row.get(2)?,
                    adapter_version: row.get(3)?,
                    observed_version: row.get(4)?,
                    installed_at: DateTime::parse_from_rfc3339(&time_str)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(rec)
    }

    fn save_window_geometry(&self, geom: &WindowGeometry) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO window_geometry (id, schema_version, width, height, x, y, is_maximized)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                geom.schema_version,
                geom.width,
                geom.height,
                geom.x,
                geom.y,
                if geom.is_maximized { 1 } else { 0 },
            ],
        )
        .map_err(|e| format!("Failed to save window geometry: {}", e))?;
        Ok(())
    }

    fn get_window_geometry(&self) -> Result<Option<WindowGeometry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT schema_version, width, height, x, y, is_maximized FROM window_geometry WHERE id = 1")
            .map_err(|e| e.to_string())?;

        let geom = stmt
            .query_row([], |row| {
                let is_max: i64 = row.get(5)?;
                Ok(WindowGeometry {
                    schema_version: row.get(0)?,
                    width: row.get(1)?,
                    height: row.get(2)?,
                    x: row.get(3)?,
                    y: row.get(4)?,
                    is_maximized: is_max != 0,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(geom)
    }

    fn commit_bundle_install_transaction(
        &self,
        operation_id: &str,
        installed_mods: &[InstalledMod],
    ) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to begin transaction: {}", e))?;

        tx.execute(
            "UPDATE operations SET state = 'completed', updated_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), operation_id],
        )
        .map_err(|e| format!("Failed to update operation in tx: {}", e))?;

        for m in installed_mods {
            let inv_json = serde_json::to_string(&m.file_inventory).unwrap_or_default();
            tx.execute(
                "INSERT OR REPLACE INTO installed_mods (id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    m.id,
                    m.setup_id,
                    m.package_id,
                    m.unique_id,
                    m.name,
                    m.author,
                    m.version,
                    m.description,
                    m.raw_manifest,
                    m.relative_target_path,
                    inv_json,
                    m.installed_at.to_rfc3339(),
                ],
            )
            .map_err(|e| format!("Failed to insert installed_mod in tx: {}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit install transaction: {}", e))?;

        Ok(())
    }

    fn commit_install_transaction(
        &self,
        operation_id: &str,
        m: &InstalledMod,
    ) -> Result<(), String> {
        self.commit_bundle_install_transaction(operation_id, std::slice::from_ref(m))
    }

    fn commit_bundle_remove_transaction(
        &self,
        operation_id: &str,
        installed_mod_ids: &[String],
    ) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to begin transaction: {}", e))?;

        tx.execute(
            "UPDATE operations SET state = 'completed', updated_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), operation_id],
        )
        .map_err(|e| format!("Failed to update operation in tx: {}", e))?;

        for id in installed_mod_ids {
            tx.execute("DELETE FROM installed_mods WHERE id = ?", params![id])
                .map_err(|e| format!("Failed to delete mod in tx: {}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit remove transaction: {}", e))?;

        Ok(())
    }

    fn commit_remove_transaction(
        &self,
        operation_id: &str,
        installed_mod_id: &str,
    ) -> Result<(), String> {
        self.commit_bundle_remove_transaction(operation_id, &[installed_mod_id.to_string()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_in_memory_migrations_and_transactions() {
        let repo = SqliteStateRepository::new_in_memory().unwrap();

        let game = GameInstallation {
            id: "test-game".to_string(),
            canonical_root: PathBuf::from("/tmp/game"),
            platform_kind: StoreKind::SteamNative,
            detected_version: Some("1.6.9".to_string()),
            validated_at: Utc::now(),
            is_fresh: true,
            is_managed: true,
            validation_error: None,
        };
        repo.save_game(&game).unwrap();

        let setup = Setup {
            id: "test-setup".to_string(),
            game_id: "test-game".to_string(),
            display_name: "Default".to_string(),
            relative_mods_dir: "setups/test/Mods".to_string(),
            created_at: Utc::now(),
        };
        repo.save_setup(&setup).unwrap();

        let op = Operation {
            id: "op-1".to_string(),
            kind: OperationKind::ModInstall,
            state: OperationState::Prepared,
            plan_json: "{}".to_string(),
            error_json: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            schema_version: 1,
        };
        repo.save_operation(&op).unwrap();

        let mod_item = InstalledMod {
            id: "mod-1".to_string(),
            setup_id: "test-setup".to_string(),
            package_id: "pkg-1".to_string(),
            unique_id: "Author.Mod".to_string(),
            name: "Mod".to_string(),
            author: "Author".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            raw_manifest: "{}".to_string(),
            relative_target_path: "ModFolder".to_string(),
            file_inventory: vec!["Mod.dll".to_string()],
            installed_at: Utc::now(),
        };

        repo.commit_install_transaction("op-1", &mod_item).unwrap();

        let op_after = repo.get_operation("op-1").unwrap().unwrap();
        assert_eq!(op_after.state, OperationState::Completed);

        let mod_after = repo.get_installed_mod("mod-1").unwrap().unwrap();
        assert_eq!(mod_after.name, "Mod");
    }
}
