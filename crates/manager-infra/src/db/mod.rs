pub mod migrations;

use chrono::{DateTime, Utc};
use manager_app::error::{AppError, AppErrorCategory, AppResult};
use manager_app::ports::repositories::{
    AtomicMutationStore, DeploymentRepository, GameInstallationRepository, InstallCommit,
    LaunchSessionRepository, OperationRepository, PackageCatalogRepository, PreferencesRepository,
    ProfileCreateCommit, ProfileRepository, RemovalCommit, SmapiRepository, WindowGeometryDto,
};
use manager_core::deployment::{
    DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{
    AcquisitionId, ArtifactHash, DeploymentId, GameInstallationId, LaunchSessionId, ModUniqueId,
    OperationId, PackageComponentId, ProfileComponentId, ProfileId,
};
use manager_core::launch::{LaunchMode, LaunchSession, SessionState, VerificationResult};
use manager_core::manifest::Manifest;
use manager_core::operation::{
    AccessMode, Operation, OperationEffect, OperationKind, OperationResource, OperationState,
    OperationStep, OperationStepState, ResourceKind,
};
use manager_core::package::{Acquisition, AcquisitionSource, PackageArtifact, PackageComponent};
use manager_core::profile::{
    AppContext, GameProfileContext, OnboardingDisposition, Profile, ProfileState,
};
use manager_core::smapi::ManagedSmapiInstallation;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

fn map_db_err(e: impl std::fmt::Display) -> AppError {
    AppError::new("DB_ERROR", AppErrorCategory::Storage, e.to_string())
}

pub(crate) fn parse_db_datetime(s: &str, col: usize) -> Result<DateTime<Utc>, rusqlite::Error> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    Err(rusqlite::Error::FromSqlConversionFailure(
        col,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse datetime '{}'", s),
        )),
    ))
}

pub(crate) fn parse_opt_db_datetime(
    s: Option<&str>,
    col: usize,
) -> Result<Option<DateTime<Utc>>, rusqlite::Error> {
    match s {
        Some(val) if !val.is_empty() => parse_db_datetime(val, col).map(Some),
        _ => Ok(None),
    }
}

fn op_state_to_str(state: OperationState) -> &'static str {
    match state {
        OperationState::Draft => "draft",
        OperationState::Prepared => "prepared",
        OperationState::Running => "running",
        OperationState::Committing => "committing",
        OperationState::Succeeded => "succeeded",
        OperationState::Failed => "failed",
        OperationState::CancellationRequested => "cancellation_requested",
        OperationState::Cancelling => "cancelling",
        OperationState::Cancelled => "cancelled",
        OperationState::RollingBack => "rolling_back",
        OperationState::RolledBack => "rolled_back",
        OperationState::RecoveryRequired => "recovery_required",
    }
}

fn op_state_from_str(s: &str) -> OperationState {
    match s {
        "draft" => OperationState::Draft,
        "prepared" => OperationState::Prepared,
        "running" => OperationState::Running,
        "committing" => OperationState::Committing,
        "succeeded" => OperationState::Succeeded,
        "cancellation_requested" => OperationState::CancellationRequested,
        "cancelling" => OperationState::Cancelling,
        "cancelled" => OperationState::Cancelled,
        "rolling_back" => OperationState::RollingBack,
        "rolled_back" => OperationState::RolledBack,
        "recovery_required" => OperationState::RecoveryRequired,
        _ => OperationState::Failed,
    }
}

fn op_kind_to_str(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::SmapiSetup => "smapi_setup",
        OperationKind::ModInstall => "mod_install",
        OperationKind::ModRemove => "mod_remove",
        OperationKind::GameLaunch => "game_launch",
        OperationKind::ProfileCreate => "profile_create",
        OperationKind::ProfileDelete => "profile_delete",
    }
}

fn op_kind_from_str(s: &str) -> OperationKind {
    match s {
        "mod_install" => OperationKind::ModInstall,
        "mod_remove" => OperationKind::ModRemove,
        "game_launch" => OperationKind::GameLaunch,
        "profile_create" => OperationKind::ProfileCreate,
        "profile_delete" => OperationKind::ProfileDelete,
        _ => OperationKind::SmapiSetup,
    }
}

#[derive(Clone)]
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

        migrations::run_migrations(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn new_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("Failed to open in-memory SQLite db: {}", e))?;

        conn.execute("PRAGMA foreign_keys = ON;", [])
            .map_err(|e| format!("Failed to enable foreign keys: {}", e))?;

        migrations::run_migrations(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

// ---------------------------------------------------------------------------
// GameInstallationRepository
// ---------------------------------------------------------------------------
impl GameInstallationRepository for SqliteStateRepository {
    fn save_game(&self, game: &GameInstallation) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let os_str = match game.operating_system {
            OperatingSystem::Linux => "linux",
            OperatingSystem::Windows => "windows",
            OperatingSystem::MacOS => "macos",
        };
        let storefront_str = match game.storefront {
            Storefront::Steam => "steam",
            Storefront::Gog => "gog",
            Storefront::Manual => "manual",
            Storefront::Unknown => "unknown",
        };
        let mode_str = match game.management_mode {
            ManagementMode::Managed => "managed",
            ManagementMode::ExternalUnmanaged => "external_unmanaged",
        };

        conn.execute(
            "INSERT INTO game_installations (id, canonical_root, platform_kind, detected_version, validated_at, is_fresh, validation_error, operating_system, storefront, management_mode, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                canonical_root=excluded.canonical_root,
                operating_system=excluded.operating_system,
                storefront=excluded.storefront,
                management_mode=excluded.management_mode",
            params![
                game.id.to_string(),
                game.canonical_root.to_string_lossy().to_string(),
                storefront_str,
                None::<String>,
                game.created_at.to_rfc3339(),
                1,
                None::<String>,
                os_str,
                storefront_str,
                mode_str,
                game.created_at.to_rfc3339(),
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_game(&self, id: &GameInstallationId) -> AppResult<Option<GameInstallation>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, canonical_root, operating_system, storefront, management_mode, created_at
                 FROM game_installations WHERE id = ?1",
            )
            .map_err(map_db_err)?;

        let game = stmt
            .query_row(params![id.to_string()], |row| {
                let id_str: String = row.get(0)?;
                let root_str: String = row.get(1)?;
                let os_str: String = row.get(2)?;
                let store_str: String = row.get(3)?;
                let mode_str: String = row.get(4)?;
                let created_str: String = row.get(5)?;

                let parsed_id = GameInstallationId::from_str(&id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let created_at = DateTime::parse_from_rfc3339(&created_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let operating_system = match os_str.as_str() {
                    "windows" => OperatingSystem::Windows,
                    "macos" => OperatingSystem::MacOS,
                    _ => OperatingSystem::Linux,
                };
                let storefront = match store_str.as_str() {
                    "gog" => Storefront::Gog,
                    "manual" => Storefront::Manual,
                    "unknown" => Storefront::Unknown,
                    _ => Storefront::Steam,
                };
                let management_mode = match mode_str.as_str() {
                    "external_unmanaged" => ManagementMode::ExternalUnmanaged,
                    _ => ManagementMode::Managed,
                };

                Ok(GameInstallation {
                    id: parsed_id,
                    canonical_root: PathBuf::from(root_str),
                    operating_system,
                    storefront,
                    management_mode,
                    created_at,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(game)
    }

    fn list_games(&self) -> AppResult<Vec<GameInstallation>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, canonical_root, operating_system, storefront, management_mode, created_at
                 FROM game_installations ORDER BY created_at ASC",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let root_str: String = row.get(1)?;
                let os_str: String = row.get(2)?;
                let store_str: String = row.get(3)?;
                let mode_str: String = row.get(4)?;
                let created_str: String = row.get(5)?;

                let parsed_id = GameInstallationId::from_str(&id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let created_at = DateTime::parse_from_rfc3339(&created_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let operating_system = match os_str.as_str() {
                    "windows" => OperatingSystem::Windows,
                    "macos" => OperatingSystem::MacOS,
                    _ => OperatingSystem::Linux,
                };
                let storefront = match store_str.as_str() {
                    "gog" => Storefront::Gog,
                    "manual" => Storefront::Manual,
                    "unknown" => Storefront::Unknown,
                    _ => Storefront::Steam,
                };
                let management_mode = match mode_str.as_str() {
                    "external_unmanaged" => ManagementMode::ExternalUnmanaged,
                    _ => ManagementMode::Managed,
                };

                Ok(GameInstallation {
                    id: parsed_id,
                    canonical_root: PathBuf::from(root_str),
                    operating_system,
                    storefront,
                    management_mode,
                    created_at,
                })
            })
            .map_err(map_db_err)?;

        let mut games = Vec::new();
        for r in rows {
            games.push(r.map_err(map_db_err)?);
        }
        Ok(games)
    }

    fn get_app_context(&self) -> AppResult<AppContext> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT active_game_installation_id, onboarding_disposition
                 FROM app_context WHERE id = 1",
            )
            .map_err(map_db_err)?;

        let ctx = stmt
            .query_row([], |row| {
                let game_str: Option<String> = row.get(0)?;
                let disp_str: String = row.get(1)?;

                let active_game_installation_id =
                    game_str.and_then(|s| GameInstallationId::from_str(&s).ok());
                let onboarding_disposition = match disp_str.as_str() {
                    "completed" => OnboardingDisposition::Completed,
                    "skipped" => OnboardingDisposition::Skipped,
                    _ => OnboardingDisposition::NotStarted,
                };

                Ok(AppContext {
                    active_game_installation_id,
                    onboarding_disposition,
                })
            })
            .optional()
            .map_err(map_db_err)?
            .unwrap_or_default();

        Ok(ctx)
    }

    fn save_app_context(&self, ctx: &AppContext) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let disp_str = match ctx.onboarding_disposition {
            OnboardingDisposition::Completed => "completed",
            OnboardingDisposition::Skipped => "skipped",
            OnboardingDisposition::NotStarted => "not_started",
        };

        conn.execute(
            "INSERT INTO app_context (id, active_game_installation_id, onboarding_disposition, updated_at)
             VALUES (1, ?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
             ON CONFLICT(id) DO UPDATE SET
                active_game_installation_id=excluded.active_game_installation_id,
                onboarding_disposition=excluded.onboarding_disposition,
                updated_at=excluded.updated_at",
            params![
                ctx.active_game_installation_id.as_ref().map(|id| id.to_string()),
                disp_str,
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ProfileRepository
// ---------------------------------------------------------------------------
impl ProfileRepository for SqliteStateRepository {
    fn save_profile(&self, profile: &Profile) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let state_str = match profile.state {
            ProfileState::Active => "active",
            ProfileState::Archived => "archived",
            ProfileState::Corrupted => "corrupted",
        };

        conn.execute(
            "INSERT INTO profiles (id, game_installation_id, name, description, revision, created_at, updated_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name,
                description=excluded.description,
                revision=excluded.revision,
                updated_at=excluded.updated_at,
                state=excluded.state",
            params![
                profile.id.to_string(),
                profile.game_installation_id.to_string(),
                profile.name,
                profile.description,
                profile.revision,
                profile.created_at.to_rfc3339(),
                profile.updated_at.to_rfc3339(),
                state_str,
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_profile(&self, id: &ProfileId) -> AppResult<Option<Profile>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, game_installation_id, name, description, revision, created_at, updated_at, state
                 FROM profiles WHERE id = ?1",
            )
            .map_err(map_db_err)?;

        let profile = stmt
            .query_row(params![id.to_string()], |row| {
                let pid_str: String = row.get(0)?;
                let gid_str: String = row.get(1)?;
                let name: String = row.get(2)?;
                let description: Option<String> = row.get(3)?;
                let revision: u64 = row.get(4)?;
                let created_str: String = row.get(5)?;
                let updated_str: String = row.get(6)?;
                let state_str: String = row.get(7)?;

                let id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let game_installation_id = GameInstallationId::from_str(&gid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let created_at = DateTime::parse_from_rfc3339(&created_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let state = match state_str.as_str() {
                    "archived" => ProfileState::Archived,
                    "corrupted" => ProfileState::Corrupted,
                    _ => ProfileState::Active,
                };

                Ok(Profile {
                    id,
                    game_installation_id,
                    name,
                    description,
                    revision,
                    created_at,
                    updated_at,
                    state,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(profile)
    }

    fn list_profiles(&self, game_id: &GameInstallationId) -> AppResult<Vec<Profile>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, game_installation_id, name, description, revision, created_at, updated_at, state
                 FROM profiles WHERE game_installation_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![game_id.to_string()], |row| {
                let pid_str: String = row.get(0)?;
                let gid_str: String = row.get(1)?;
                let name: String = row.get(2)?;
                let description: Option<String> = row.get(3)?;
                let revision: u64 = row.get(4)?;
                let created_str: String = row.get(5)?;
                let updated_str: String = row.get(6)?;
                let state_str: String = row.get(7)?;

                let id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let game_installation_id = GameInstallationId::from_str(&gid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let created_at = DateTime::parse_from_rfc3339(&created_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let state = match state_str.as_str() {
                    "archived" => ProfileState::Archived,
                    "corrupted" => ProfileState::Corrupted,
                    _ => ProfileState::Active,
                };

                Ok(Profile {
                    id,
                    game_installation_id,
                    name,
                    description,
                    revision,
                    created_at,
                    updated_at,
                    state,
                })
            })
            .map_err(map_db_err)?;

        let mut profiles = Vec::new();
        for r in rows {
            profiles.push(r.map_err(map_db_err)?);
        }
        Ok(profiles)
    }

    fn get_game_profile_context(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Option<GameProfileContext>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT game_installation_id, active_profile_id, default_profile_id, last_active_profile_id
                 FROM game_profile_context WHERE game_installation_id = ?1",
            )
            .map_err(map_db_err)?;

        let ctx = stmt
            .query_row(params![game_id.to_string()], |row| {
                let gid_str: String = row.get(0)?;
                let active_str: Option<String> = row.get(1)?;
                let def_str: Option<String> = row.get(2)?;
                let last_str: Option<String> = row.get(3)?;

                let parsed_gid = GameInstallationId::from_str(&gid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let active_profile_id = active_str.and_then(|s| ProfileId::from_str(&s).ok());
                let default_profile_id = def_str.and_then(|s| ProfileId::from_str(&s).ok());
                let last_active_profile_id = last_str.and_then(|s| ProfileId::from_str(&s).ok());

                Ok(GameProfileContext {
                    game_installation_id: parsed_gid,
                    active_profile_id,
                    default_profile_id,
                    last_active_profile_id,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(ctx)
    }

    fn save_game_profile_context(&self, ctx: &GameProfileContext) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        conn.execute(
            "INSERT INTO game_profile_context (game_installation_id, active_profile_id, default_profile_id, last_active_profile_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
             ON CONFLICT(game_installation_id) DO UPDATE SET
                active_profile_id=excluded.active_profile_id,
                default_profile_id=excluded.default_profile_id,
                last_active_profile_id=excluded.last_active_profile_id,
                updated_at=excluded.updated_at",
            params![
                ctx.game_installation_id.to_string(),
                ctx.active_profile_id.as_ref().map(|id| id.to_string()),
                ctx.default_profile_id.as_ref().map(|id| id.to_string()),
                ctx.last_active_profile_id.as_ref().map(|id| id.to_string()),
            ],
        )
        .map_err(map_db_err)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PackageCatalogRepository
// ---------------------------------------------------------------------------
impl PackageCatalogRepository for SqliteStateRepository {
    fn save_artifact(&self, artifact: &PackageArtifact) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        conn.execute(
            "INSERT INTO packages (hash, byte_size, created_at, original_filename, source_kind, storage_relative_path, first_seen_at)
             VALUES (?1, ?2, ?3, '', 'artifact', ?4, ?5)
             ON CONFLICT(hash) DO NOTHING",
            params![
                artifact.hash.as_str(),
                artifact.byte_size,
                artifact.first_seen_at.to_rfc3339(),
                artifact.storage_relative_path,
                artifact.first_seen_at.to_rfc3339(),
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_artifact(&self, hash: &ArtifactHash) -> AppResult<Option<PackageArtifact>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare("SELECT hash, byte_size, storage_relative_path, first_seen_at FROM packages WHERE hash = ?1")
            .map_err(map_db_err)?;

        let artifact = stmt
            .query_row(params![hash.as_str()], |row| {
                let h_str: String = row.get(0)?;
                let byte_size: u64 = row.get(1)?;
                let path: String = row.get(2)?;
                let date_str: String = row.get(3)?;
                let first_seen_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(PackageArtifact {
                    hash: ArtifactHash::new(h_str),
                    byte_size,
                    storage_relative_path: path,
                    first_seen_at,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(artifact)
    }

    fn list_artifacts(&self) -> AppResult<Vec<PackageArtifact>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare("SELECT hash, byte_size, storage_relative_path, first_seen_at FROM packages ORDER BY first_seen_at DESC")
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map([], |row| {
                let h_str: String = row.get(0)?;
                let byte_size: u64 = row.get(1)?;
                let path: String = row.get(2)?;
                let date_str: String = row.get(3)?;
                let first_seen_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(PackageArtifact {
                    hash: ArtifactHash::new(h_str),
                    byte_size,
                    storage_relative_path: path,
                    first_seen_at,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn save_acquisition(&self, acq: &Acquisition) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let source_str = match acq.source {
            AcquisitionSource::LocalFile => "local_file",
            AcquisitionSource::DirectUrl => "direct_url",
            AcquisitionSource::Provider => "provider",
            AcquisitionSource::ManualReference => "manual_reference",
        };

        conn.execute(
            "INSERT INTO acquisitions (id, artifact_hash, source, original_filename, acquired_at, expected_hash, source_metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO NOTHING",
            params![
                acq.id.to_string(),
                acq.artifact_hash.as_str(),
                source_str,
                acq.original_filename,
                acq.acquired_at.to_rfc3339(),
                acq.expected_hash.as_ref().map(|h| h.as_str()),
                acq.source_metadata,
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_acquisitions_for_artifact(
        &self,
        artifact_hash: &ArtifactHash,
    ) -> AppResult<Vec<Acquisition>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, artifact_hash, source, original_filename, acquired_at, expected_hash, source_metadata
                 FROM acquisitions WHERE artifact_hash = ?1 ORDER BY acquired_at DESC",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![artifact_hash.as_str()], |row| {
                let id_str: String = row.get(0)?;
                let hash_str: String = row.get(1)?;
                let src_str: String = row.get(2)?;
                let filename: String = row.get(3)?;
                let date_str: String = row.get(4)?;
                let expected_hash_str: Option<String> = row.get(5)?;
                let source_metadata: Option<String> = row.get(6)?;

                let id = AcquisitionId::from_str(&id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let acquired_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let source = match src_str.as_str() {
                    "direct_url" => AcquisitionSource::DirectUrl,
                    "provider" => AcquisitionSource::Provider,
                    "manual_reference" => AcquisitionSource::ManualReference,
                    _ => AcquisitionSource::LocalFile,
                };

                Ok(Acquisition {
                    id,
                    artifact_hash: ArtifactHash::new(hash_str),
                    source,
                    original_filename: filename,
                    acquired_at,
                    expected_hash: expected_hash_str.map(ArtifactHash::new),
                    source_metadata,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn save_package_component(&self, comp: &PackageComponent) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let manifest_json = serde_json::to_string(&comp.manifest)
            .map_err(|e| AppError::internal("Failed to serialize manifest", e.to_string()))?;

        conn.execute(
            "INSERT INTO package_components (id, artifact_hash, unique_id, name, author, version, description, relative_component_root, raw_manifest, manifest_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO NOTHING",
            params![
                comp.id.to_string(),
                comp.artifact_hash.as_str(),
                comp.unique_id.as_str(),
                comp.name,
                comp.author,
                comp.version,
                comp.description,
                comp.relative_component_root,
                comp.raw_manifest,
                manifest_json,
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_package_component(
        &self,
        id: &PackageComponentId,
    ) -> AppResult<Option<PackageComponent>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, artifact_hash, unique_id, name, author, version, description, relative_component_root, raw_manifest, manifest_json
                 FROM package_components WHERE id = ?1",
            )
            .map_err(map_db_err)?;

        let comp = stmt
            .query_row(params![id.to_string()], |row| {
                let cid_str: String = row.get(0)?;
                let hash_str: String = row.get(1)?;
                let unique_id: String = row.get(2)?;
                let name: String = row.get(3)?;
                let author: String = row.get(4)?;
                let version: String = row.get(5)?;
                let description: Option<String> = row.get(6)?;
                let root: String = row.get(7)?;
                let raw_manifest: String = row.get(8)?;
                let manifest_json: String = row.get(9)?;

                let parsed_cid = PackageComponentId::from_str(&cid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let manifest: Manifest = serde_json::from_str(&manifest_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        9,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                Ok(PackageComponent {
                    id: parsed_cid,
                    artifact_hash: ArtifactHash::new(hash_str),
                    unique_id: ModUniqueId::new(unique_id),
                    name,
                    author,
                    version,
                    description,
                    relative_component_root: root,
                    raw_manifest,
                    manifest,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(comp)
    }

    fn list_components_for_artifact(
        &self,
        artifact_hash: &ArtifactHash,
    ) -> AppResult<Vec<PackageComponent>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, artifact_hash, unique_id, name, author, version, description, relative_component_root, raw_manifest, manifest_json
                 FROM package_components WHERE artifact_hash = ?1",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![artifact_hash.as_str()], |row| {
                let cid_str: String = row.get(0)?;
                let hash_str: String = row.get(1)?;
                let unique_id: String = row.get(2)?;
                let name: String = row.get(3)?;
                let author: String = row.get(4)?;
                let version: String = row.get(5)?;
                let description: Option<String> = row.get(6)?;
                let root: String = row.get(7)?;
                let raw_manifest: String = row.get(8)?;
                let manifest_json: String = row.get(9)?;

                let parsed_cid = PackageComponentId::from_str(&cid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let manifest: Manifest = serde_json::from_str(&manifest_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        9,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                Ok(PackageComponent {
                    id: parsed_cid,
                    artifact_hash: ArtifactHash::new(hash_str),
                    unique_id: ModUniqueId::new(unique_id),
                    name,
                    author,
                    version,
                    description,
                    relative_component_root: root,
                    raw_manifest,
                    manifest,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }
}

// ---------------------------------------------------------------------------
// DeploymentRepository
// ---------------------------------------------------------------------------
impl DeploymentRepository for SqliteStateRepository {
    fn save_deployment(&self, dep: &ProfileDeployment) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let state_str = match dep.state {
            DeploymentState::Present => "present",
            DeploymentState::Disabled => "disabled",
            DeploymentState::Missing => "missing",
            DeploymentState::ExternallyModified => "externally_modified",
            DeploymentState::Quarantined => "quarantined",
        };

        conn.execute(
            "INSERT INTO profile_deployments (id, profile_id, artifact_hash, root_relative_path, installed_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                state=excluded.state,
                root_relative_path=excluded.root_relative_path",
            params![
                dep.id.to_string(),
                dep.profile_id.to_string(),
                dep.artifact_hash.as_str(),
                dep.root_relative_path,
                dep.installed_at.to_rfc3339(),
                state_str,
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_deployment(&self, id: &DeploymentId) -> AppResult<Option<ProfileDeployment>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, profile_id, artifact_hash, root_relative_path, installed_at, state
                 FROM profile_deployments WHERE id = ?1",
            )
            .map_err(map_db_err)?;

        let dep = stmt
            .query_row(params![id.to_string()], |row| {
                let did_str: String = row.get(0)?;
                let pid_str: String = row.get(1)?;
                let hash_str: String = row.get(2)?;
                let path_str: String = row.get(3)?;
                let date_str: String = row.get(4)?;
                let state_str: String = row.get(5)?;

                let parsed_did = DeploymentId::from_str(&did_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let installed_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let state = match state_str.as_str() {
                    "disabled" => DeploymentState::Disabled,
                    "missing" => DeploymentState::Missing,
                    "externally_modified" => DeploymentState::ExternallyModified,
                    "quarantined" => DeploymentState::Quarantined,
                    _ => DeploymentState::Present,
                };

                Ok(ProfileDeployment {
                    id: parsed_did,
                    profile_id,
                    artifact_hash: ArtifactHash::new(hash_str),
                    root_relative_path: path_str,
                    installed_at,
                    state,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(dep)
    }

    fn list_deployments_for_profile(
        &self,
        profile_id: &ProfileId,
    ) -> AppResult<Vec<ProfileDeployment>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, profile_id, artifact_hash, root_relative_path, installed_at, state
                 FROM profile_deployments WHERE profile_id = ?1",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![profile_id.to_string()], |row| {
                let did_str: String = row.get(0)?;
                let pid_str: String = row.get(1)?;
                let hash_str: String = row.get(2)?;
                let path_str: String = row.get(3)?;
                let date_str: String = row.get(4)?;
                let state_str: String = row.get(5)?;

                let parsed_did = DeploymentId::from_str(&did_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let installed_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let state = match state_str.as_str() {
                    "disabled" => DeploymentState::Disabled,
                    "missing" => DeploymentState::Missing,
                    "externally_modified" => DeploymentState::ExternallyModified,
                    "quarantined" => DeploymentState::Quarantined,
                    _ => DeploymentState::Present,
                };

                Ok(ProfileDeployment {
                    id: parsed_did,
                    profile_id,
                    artifact_hash: ArtifactHash::new(hash_str),
                    root_relative_path: path_str,
                    installed_at,
                    state,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn save_profile_component(&self, comp: &ProfileComponent) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let reason_str = match comp.installed_reason {
            InstalledReason::Direct => "direct",
            InstalledReason::Dependency => "dependency",
            InstalledReason::BundleCompanion => "bundle_companion",
        };
        conn.execute(
            "INSERT INTO profile_components (id, profile_id, deployment_id, package_component_id, enabled, installed_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                enabled=excluded.enabled,
                installed_reason=excluded.installed_reason",
            params![
                comp.id.to_string(),
                comp.profile_id.to_string(),
                comp.deployment_id.to_string(),
                comp.package_component_id.to_string(),
                if comp.enabled { 1 } else { 0 },
                reason_str,
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_profile_component(
        &self,
        id: &ProfileComponentId,
    ) -> AppResult<Option<ProfileComponent>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, profile_id, deployment_id, package_component_id, enabled, installed_reason
                 FROM profile_components WHERE id = ?1",
            )
            .map_err(map_db_err)?;

        let comp = stmt
            .query_row(params![id.to_string()], |row| {
                let id_str: String = row.get(0)?;
                let pid_str: String = row.get(1)?;
                let did_str: String = row.get(2)?;
                let pcid_str: String = row.get(3)?;
                let enabled_int: i32 = row.get(4)?;
                let reason_str: String = row.get(5)?;

                let parsed_id = ProfileComponentId::from_str(&id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let deployment_id = DeploymentId::from_str(&did_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let package_component_id =
                    PackageComponentId::from_str(&pcid_str).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let installed_reason = match reason_str.as_str() {
                    "dependency" => InstalledReason::Dependency,
                    "bundle_companion" => InstalledReason::BundleCompanion,
                    _ => InstalledReason::Direct,
                };

                Ok(ProfileComponent {
                    id: parsed_id,
                    profile_id,
                    deployment_id,
                    package_component_id,
                    enabled: enabled_int != 0,
                    installed_reason,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(comp)
    }

    fn list_profile_components(&self, profile_id: &ProfileId) -> AppResult<Vec<ProfileComponent>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, profile_id, deployment_id, package_component_id, enabled, installed_reason
                 FROM profile_components WHERE profile_id = ?1",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![profile_id.to_string()], |row| {
                let id_str: String = row.get(0)?;
                let pid_str: String = row.get(1)?;
                let did_str: String = row.get(2)?;
                let pcid_str: String = row.get(3)?;
                let enabled_int: i32 = row.get(4)?;
                let reason_str: String = row.get(5)?;

                let parsed_id = ProfileComponentId::from_str(&id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let deployment_id = DeploymentId::from_str(&did_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let package_component_id =
                    PackageComponentId::from_str(&pcid_str).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                let installed_reason = match reason_str.as_str() {
                    "dependency" => InstalledReason::Dependency,
                    "bundle_companion" => InstalledReason::BundleCompanion,
                    _ => InstalledReason::Direct,
                };

                Ok(ProfileComponent {
                    id: parsed_id,
                    profile_id,
                    deployment_id,
                    package_component_id,
                    enabled: enabled_int != 0,
                    installed_reason,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn delete_profile_component(&self, id: &ProfileComponentId) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        conn.execute(
            "DELETE FROM profile_components WHERE id = ?1",
            params![id.to_string()],
        )
        .map_err(map_db_err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// OperationRepository
// ---------------------------------------------------------------------------
impl OperationRepository for SqliteStateRepository {
    fn save_operation(&self, op: &Operation) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let kind_str = op_kind_to_str(op.kind);
        let state_str = op_state_to_str(op.state);

        conn.execute(
            "INSERT INTO operations (id, kind, state, plan_json, error_json, created_at, updated_at, schema_version, game_installation_id, profile_id, expected_profile_revision, plan_schema_version, progress_current, progress_total, error_code, cancellation_requested, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(id) DO UPDATE SET
                state=excluded.state,
                error_json=excluded.error_json,
                updated_at=excluded.updated_at,
                expected_profile_revision=excluded.expected_profile_revision,
                progress_current=excluded.progress_current,
                progress_total=excluded.progress_total,
                error_code=excluded.error_code,
                cancellation_requested=excluded.cancellation_requested,
                completed_at=excluded.completed_at",
            params![
                op.id.to_string(),
                kind_str,
                state_str,
                op.plan_json,
                op.error_json,
                op.created_at.to_rfc3339(),
                op.updated_at.to_rfc3339(),
                op.plan_schema_version,
                op.game_installation_id.as_ref().map(|id| id.to_string()),
                op.profile_id.as_ref().map(|id| id.to_string()),
                op.expected_profile_revision,
                op.plan_schema_version,
                op.progress_current,
                op.progress_total,
                op.error_code,
                if op.cancellation_requested { 1 } else { 0 },
                op.completed_at.as_ref().map(|t| t.to_rfc3339()),
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_operation(&self, id: &OperationId) -> AppResult<Option<Operation>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, state, plan_json, error_json, created_at, updated_at, schema_version, game_installation_id, profile_id, expected_profile_revision, progress_current, progress_total, error_code, cancellation_requested, completed_at
                 FROM operations WHERE id = ?1",
            )
            .map_err(map_db_err)?;

        let op = stmt
            .query_row(params![id.to_string()], |row| {
                let id_str: String = row.get(0)?;
                let kind_str: String = row.get(1)?;
                let state_str: String = row.get(2)?;
                let plan_json: String = row.get(3)?;
                let error_json: Option<String> = row.get(4)?;
                let created_str: String = row.get(5)?;
                let updated_str: String = row.get(6)?;
                let schema_version: u32 = row.get(7)?;
                let gid_str: Option<String> = row.get(8)?;
                let pid_str: Option<String> = row.get(9)?;
                let expected_profile_revision: Option<u64> = row.get(10)?;
                let progress_current: Option<u32> = row.get(11)?;
                let progress_total: Option<u32> = row.get(12)?;
                let error_code: Option<String> = row.get(13)?;
                let cancel_req: i32 = row.get(14)?;
                let completed_str: Option<String> = row.get(15)?;

                let parsed_id = OperationId::from_str(&id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let created_at = parse_db_datetime(&created_str, 5)?;
                let updated_at = parse_db_datetime(&updated_str, 6)?;
                let completed_at = parse_opt_db_datetime(completed_str.as_deref(), 15)?;

                let game_installation_id =
                    gid_str.and_then(|s| GameInstallationId::from_str(&s).ok());
                let profile_id = pid_str.and_then(|s| ProfileId::from_str(&s).ok());

                let kind = op_kind_from_str(&kind_str);
                let state = op_state_from_str(&state_str);

                Ok(Operation {
                    id: parsed_id,
                    kind,
                    state,
                    game_installation_id,
                    profile_id,
                    expected_profile_revision,
                    plan_schema_version: schema_version,
                    plan_json,
                    progress_current,
                    progress_total,
                    error_code,
                    error_json,
                    cancellation_requested: cancel_req != 0,
                    created_at,
                    updated_at,
                    completed_at,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(op)
    }

    fn update_operation_state(
        &self,
        id: &OperationId,
        state: OperationState,
        error_code: Option<String>,
        error_json: Option<String>,
    ) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let state_str = op_state_to_str(state);

        conn.execute(
            "UPDATE operations SET state = ?1, error_code = COALESCE(?2, error_code), error_json = COALESCE(?3, error_json), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?4",
            params![state_str, error_code, error_json, id.to_string()],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn list_unresolved_operations(&self) -> AppResult<Vec<Operation>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, state, plan_json, error_json, created_at, updated_at, schema_version, game_installation_id, profile_id, expected_profile_revision, progress_current, progress_total, error_code, cancellation_requested, completed_at
                 FROM operations
                 WHERE state NOT IN ('succeeded', 'cancelled', 'rolled_back')
                 ORDER BY created_at ASC",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let kind_str: String = row.get(1)?;
                let state_str: String = row.get(2)?;
                let plan_json: String = row.get(3)?;
                let error_json: Option<String> = row.get(4)?;
                let created_str: String = row.get(5)?;
                let updated_str: String = row.get(6)?;
                let schema_version: u32 = row.get(7)?;
                let gid_str: Option<String> = row.get(8)?;
                let pid_str: Option<String> = row.get(9)?;
                let expected_profile_revision: Option<u64> = row.get(10)?;
                let progress_current: Option<u32> = row.get(11)?;
                let progress_total: Option<u32> = row.get(12)?;
                let error_code: Option<String> = row.get(13)?;
                let cancel_req: i32 = row.get(14)?;
                let completed_str: Option<String> = row.get(15)?;

                let parsed_id = OperationId::from_str(&id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let created_at = parse_db_datetime(&created_str, 5)?;
                let updated_at = parse_db_datetime(&updated_str, 6)?;
                let completed_at = parse_opt_db_datetime(completed_str.as_deref(), 15)?;

                let game_installation_id =
                    gid_str.and_then(|s| GameInstallationId::from_str(&s).ok());
                let profile_id = pid_str.and_then(|s| ProfileId::from_str(&s).ok());

                let kind = op_kind_from_str(&kind_str);
                let state = op_state_from_str(&state_str);

                Ok(Operation {
                    id: parsed_id,
                    kind,
                    state,
                    game_installation_id,
                    profile_id,
                    expected_profile_revision,
                    plan_schema_version: schema_version,
                    plan_json,
                    progress_current,
                    progress_total,
                    error_code,
                    error_json,
                    cancellation_requested: cancel_req != 0,
                    created_at,
                    updated_at,
                    completed_at,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn list_operations_for_profile(&self, profile_id: &ProfileId) -> AppResult<Vec<Operation>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT o.id, o.kind, o.state, o.plan_json, o.error_json, o.created_at, o.updated_at, o.schema_version, o.game_installation_id, o.profile_id, o.expected_profile_revision, o.progress_current, o.progress_total, o.error_code, o.cancellation_requested, o.completed_at
                 FROM operations o
                 LEFT JOIN operation_resources r ON o.id = r.operation_id
                 WHERE o.profile_id = ?1 OR r.resource_id = ?1 OR o.plan_json LIKE ?2
                 ORDER BY o.created_at DESC",
            )
            .map_err(map_db_err)?;

        let like_profile = format!("%{}%", profile_id);
        let rows = stmt
            .query_map(params![profile_id.to_string(), like_profile], |row| {
                let id_str: String = row.get(0)?;
                let kind_str: String = row.get(1)?;
                let state_str: String = row.get(2)?;
                let plan_json: String = row.get(3)?;
                let error_json: Option<String> = row.get(4)?;
                let created_str: String = row.get(5)?;
                let updated_str: String = row.get(6)?;
                let schema_version: u32 = row.get(7)?;
                let gid_str: Option<String> = row.get(8)?;
                let pid_str: Option<String> = row.get(9)?;
                let expected_profile_revision: Option<u64> = row.get(10)?;
                let progress_current: Option<u32> = row.get(11)?;
                let progress_total: Option<u32> = row.get(12)?;
                let error_code: Option<String> = row.get(13)?;
                let cancel_req: i32 = row.get(14)?;
                let completed_str: Option<String> = row.get(15)?;

                let parsed_id = OperationId::from_str(&id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let created_at = parse_db_datetime(&created_str, 5)?;
                let updated_at = parse_db_datetime(&updated_str, 6)?;
                let completed_at = parse_opt_db_datetime(completed_str.as_deref(), 15)?;

                let game_installation_id =
                    gid_str.and_then(|s| GameInstallationId::from_str(&s).ok());
                let profile_id = pid_str.and_then(|s| ProfileId::from_str(&s).ok());

                let kind = op_kind_from_str(&kind_str);
                let state = op_state_from_str(&state_str);

                Ok(Operation {
                    id: parsed_id,
                    kind,
                    state,
                    game_installation_id,
                    profile_id,
                    expected_profile_revision,
                    plan_schema_version: schema_version,
                    plan_json,
                    progress_current,
                    progress_total,
                    error_code,
                    error_json,
                    cancellation_requested: cancel_req != 0,
                    created_at,
                    updated_at,
                    completed_at,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn save_operation_step(&self, step: &OperationStep) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let state_str = match step.state {
            OperationStepState::Pending => "pending",
            OperationStepState::Running => "running",
            OperationStepState::Completed => "completed",
            OperationStepState::Failed => "failed",
        };

        conn.execute(
            "INSERT INTO operation_steps (operation_id, step_index, step_kind, state, payload_json, started_at, completed_at, error_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(operation_id, step_index) DO UPDATE SET
                state=excluded.state,
                completed_at=excluded.completed_at,
                error_json=excluded.error_json",
            params![
                step.operation_id.to_string(),
                step.step_index,
                step.step_kind,
                state_str,
                step.payload_json,
                step.started_at.as_ref().map(|t| t.to_rfc3339()),
                step.completed_at.as_ref().map(|t| t.to_rfc3339()),
                step.error_json,
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn list_operation_steps(&self, op_id: &OperationId) -> AppResult<Vec<OperationStep>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT operation_id, step_index, step_kind, state, payload_json, started_at, completed_at, error_json
                 FROM operation_steps WHERE operation_id = ?1 ORDER BY step_index ASC",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![op_id.to_string()], |row| {
                let op_id_str: String = row.get(0)?;
                let step_index: u32 = row.get(1)?;
                let step_kind: String = row.get(2)?;
                let state_str: String = row.get(3)?;
                let payload_json: String = row.get(4)?;
                let started_str: Option<String> = row.get(5)?;
                let completed_str: Option<String> = row.get(6)?;
                let error_json: Option<String> = row.get(7)?;

                let parsed_op_id = OperationId::from_str(&op_id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                let started_at = parse_opt_db_datetime(started_str.as_deref(), 5)?;
                let completed_at = parse_opt_db_datetime(completed_str.as_deref(), 6)?;

                let state = match state_str.as_str() {
                    "running" => OperationStepState::Running,
                    "completed" => OperationStepState::Completed,
                    "failed" => OperationStepState::Failed,
                    _ => OperationStepState::Pending,
                };

                Ok(OperationStep {
                    operation_id: parsed_op_id,
                    step_index,
                    step_kind,
                    state,
                    payload_json,
                    started_at,
                    completed_at,
                    error_json,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn save_operation_resource(&self, res: &OperationResource) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let kind_str = match res.resource_kind {
            ResourceKind::Profile => "profile",
            ResourceKind::GameInstallation => "game_installation",
            ResourceKind::Artifact => "artifact",
        };
        let access_str = match res.access_mode {
            AccessMode::Write => "write",
            AccessMode::Read => "read",
        };

        conn.execute(
            "INSERT INTO operation_resources (operation_id, resource_kind, resource_id, access_mode)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(operation_id, resource_kind, resource_id) DO NOTHING",
            params![
                res.operation_id.to_string(),
                kind_str,
                res.resource_id,
                access_str,
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn list_operation_resources(&self, op_id: &OperationId) -> AppResult<Vec<OperationResource>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT operation_id, resource_kind, resource_id, access_mode
                 FROM operation_resources WHERE operation_id = ?1",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![op_id.to_string()], |row| {
                let op_id_str: String = row.get(0)?;
                let kind_str: String = row.get(1)?;
                let res_id: String = row.get(2)?;
                let access_str: String = row.get(3)?;

                let parsed_op_id = OperationId::from_str(&op_id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                let resource_kind = match kind_str.as_str() {
                    "profile" => ResourceKind::Profile,
                    "game_installation" => ResourceKind::GameInstallation,
                    _ => ResourceKind::Artifact,
                };
                let access_mode = match access_str.as_str() {
                    "write" | "exclusive" => AccessMode::Write,
                    _ => AccessMode::Read,
                };

                Ok(OperationResource {
                    operation_id: parsed_op_id,
                    resource_kind,
                    resource_id: res_id,
                    access_mode,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn list_unresolved_resources_for_profile(
        &self,
        profile_id: &ProfileId,
    ) -> AppResult<Vec<OperationResource>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT r.operation_id, r.resource_kind, r.resource_id, r.access_mode
                 FROM operation_resources r
                 JOIN operations o ON r.operation_id = o.id
                 WHERE o.state NOT IN ('succeeded', 'cancelled', 'rolled_back')
                   AND r.resource_id = ?1",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![profile_id.to_string()], |row| {
                let op_id_str: String = row.get(0)?;
                let kind_str: String = row.get(1)?;
                let res_id: String = row.get(2)?;
                let access_str: String = row.get(3)?;

                let parsed_op_id = OperationId::from_str(&op_id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                let resource_kind = match kind_str.as_str() {
                    "profile" => ResourceKind::Profile,
                    "game_installation" => ResourceKind::GameInstallation,
                    _ => ResourceKind::Artifact,
                };
                let access_mode = match access_str.as_str() {
                    "write" | "exclusive" => AccessMode::Write,
                    _ => AccessMode::Read,
                };

                Ok(OperationResource {
                    operation_id: parsed_op_id,
                    resource_kind,
                    resource_id: res_id,
                    access_mode,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn save_operation_effect(&self, effect: &OperationEffect) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        conn.execute(
            "INSERT INTO operation_effects (id, operation_id, profile_id, entity_type, entity_id, change_kind, before_json, after_json, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO NOTHING",
            params![
                effect.id,
                effect.operation_id.to_string(),
                effect.profile_id.as_ref().map(|id| id.to_string()),
                effect.entity_type,
                effect.entity_id,
                effect.change_kind,
                effect.before_json,
                effect.after_json,
                effect.occurred_at.to_rfc3339(),
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn list_operation_effects(&self, op_id: &OperationId) -> AppResult<Vec<OperationEffect>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, operation_id, profile_id, entity_type, entity_id, change_kind, before_json, after_json, occurred_at
                 FROM operation_effects WHERE operation_id = ?1 ORDER BY occurred_at ASC",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![op_id.to_string()], |row| {
                let id: String = row.get(0)?;
                let op_id_str: String = row.get(1)?;
                let profile_str: Option<String> = row.get(2)?;
                let entity_type: String = row.get(3)?;
                let entity_id: String = row.get(4)?;
                let change_kind: String = row.get(5)?;
                let before_json: Option<String> = row.get(6)?;
                let after_json: Option<String> = row.get(7)?;
                let date_str: String = row.get(8)?;

                let parsed_op_id = OperationId::from_str(&op_id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = profile_str.and_then(|s| ProfileId::from_str(&s).ok());
                let occurred_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            8,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(OperationEffect {
                    id,
                    operation_id: parsed_op_id,
                    profile_id,
                    entity_type,
                    entity_id,
                    change_kind,
                    before_json,
                    after_json,
                    occurred_at,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }

    fn list_recent_effects_for_profile(
        &self,
        profile_id: &ProfileId,
        limit: usize,
    ) -> AppResult<Vec<OperationEffect>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT e.id, e.operation_id, e.profile_id, e.entity_type, e.entity_id, e.change_kind, e.before_json, e.after_json, e.occurred_at
                 FROM operation_effects e
                 WHERE e.profile_id = ?1
                 ORDER BY e.occurred_at DESC LIMIT ?2",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![profile_id.to_string(), limit as i64], |row| {
                let id: String = row.get(0)?;
                let op_id_str: String = row.get(1)?;
                let profile_str: Option<String> = row.get(2)?;
                let entity_type: String = row.get(3)?;
                let entity_id: String = row.get(4)?;
                let change_kind: String = row.get(5)?;
                let before_json: Option<String> = row.get(6)?;
                let after_json: Option<String> = row.get(7)?;
                let date_str: String = row.get(8)?;

                let parsed_op_id = OperationId::from_str(&op_id_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = profile_str.and_then(|s| ProfileId::from_str(&s).ok());
                let occurred_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            8,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(OperationEffect {
                    id,
                    operation_id: parsed_op_id,
                    profile_id,
                    entity_type,
                    entity_id,
                    change_kind,
                    before_json,
                    after_json,
                    occurred_at,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }
}

// ---------------------------------------------------------------------------
// LaunchSessionRepository
// ---------------------------------------------------------------------------
impl LaunchSessionRepository for SqliteStateRepository {
    fn save_launch_session(&self, session: &LaunchSession) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let state_str = match session.state {
            SessionState::Starting => "starting",
            SessionState::RunningUnverified => "running_unverified",
            SessionState::ModLoadConfirmed => "mod_load_confirmed",
            SessionState::Exited => "exited",
            SessionState::Failed => "failed",
            SessionState::VerificationUnavailable => "verification_unavailable",
        };
        let mode_str = match session.launch_mode {
            LaunchMode::Vanilla => "vanilla",
            LaunchMode::RuntimeTest => "runtime_test",
            LaunchMode::Modded => "modded",
        };
        let expected_json = serde_json::to_string(&session.expected_mod_ids)
            .map_err(|e| AppError::internal("Failed to serialize mod IDs", e.to_string()))?;
        let verif_json = session
            .verification_result
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok());
        let baseline_json = session
            .log_baseline
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok());

        conn.execute(
            "INSERT INTO launch_sessions (id, game_id, setup_id, launched_at, ended_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json, launch_mode)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
                ended_at=excluded.ended_at,
                pid=excluded.pid,
                state=excluded.state,
                verification_result_json=excluded.verification_result_json,
                log_baseline_json=excluded.log_baseline_json",
            params![
                session.id.to_string(),
                session.game_installation_id.to_string(),
                session.profile_id.to_string(),
                session.launched_at.to_rfc3339(),
                session.ended_at.as_ref().map(|d| d.to_rfc3339()),
                session.pid,
                state_str,
                expected_json,
                session.log_baseline_time.as_ref().map(|d| d.to_rfc3339()),
                verif_json,
                baseline_json,
                mode_str,
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_launch_session(&self, id: &LaunchSessionId) -> AppResult<Option<LaunchSession>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, game_id, setup_id, launched_at, ended_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json, launch_mode
                 FROM launch_sessions WHERE id = ?1",
            )
            .map_err(map_db_err)?;

        let session = stmt
            .query_row(params![id.to_string()], |row| {
                let sid_str: String = row.get(0)?;
                let gid_str: String = row.get(1)?;
                let pid_str: String = row.get(2)?;
                let date_str: String = row.get(3)?;
                let ended_str: Option<String> = row.get(4)?;
                let pid: Option<u32> = row.get(5)?;
                let state_str: String = row.get(6)?;
                let expected_json: String = row.get(7)?;
                let baseline_time_str: Option<String> = row.get(8)?;
                let verif_json: Option<String> = row.get(9)?;
                let baseline_json: Option<String> = row.get(10)?;
                let mode_str: Option<String> = row.get(11)?;

                let parsed_sid = LaunchSessionId::from_str(&sid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let game_installation_id = GameInstallationId::from_str(&gid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let launched_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let ended_at = ended_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                });
                let expected_mod_ids: Vec<ModUniqueId> = serde_json::from_str(&expected_json)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            7,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let log_baseline_time = baseline_time_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                });
                let verification_result: Option<VerificationResult> =
                    verif_json.and_then(|v| serde_json::from_str(&v).ok());
                let log_baseline = baseline_json.and_then(|v| serde_json::from_str(&v).ok());

                let state = match state_str.as_str() {
                    "running_unverified" => SessionState::RunningUnverified,
                    "mod_load_confirmed" => SessionState::ModLoadConfirmed,
                    "exited" => SessionState::Exited,
                    "failed" => SessionState::Failed,
                    "verification_unavailable" => SessionState::VerificationUnavailable,
                    _ => SessionState::Starting,
                };

                let launch_mode = match mode_str.as_deref() {
                    Some("vanilla") => LaunchMode::Vanilla,
                    Some("runtime_test") => LaunchMode::RuntimeTest,
                    _ => LaunchMode::Modded,
                };

                Ok(LaunchSession {
                    id: parsed_sid,
                    game_installation_id,
                    profile_id,
                    launch_mode,
                    launched_at,
                    ended_at,
                    pid,
                    state,
                    expected_mod_ids,
                    log_baseline_time,
                    log_baseline,
                    verification_result,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(session)
    }

    fn get_latest_launch_session(
        &self,
        profile_id: Option<&ProfileId>,
    ) -> AppResult<Option<LaunchSession>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let query = if profile_id.is_some() {
            "SELECT id, game_id, setup_id, launched_at, ended_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json, launch_mode
             FROM launch_sessions WHERE setup_id = ?1 ORDER BY launched_at DESC LIMIT 1"
        } else {
            "SELECT id, game_id, setup_id, launched_at, ended_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json, launch_mode
             FROM launch_sessions ORDER BY launched_at DESC LIMIT 1"
        };

        let mut stmt = conn.prepare(query).map_err(map_db_err)?;

        let session = if let Some(pid) = profile_id {
            stmt.query_row(params![pid.to_string()], |row| {
                let sid_str: String = row.get(0)?;
                let gid_str: String = row.get(1)?;
                let pid_str: String = row.get(2)?;
                let date_str: String = row.get(3)?;
                let ended_str: Option<String> = row.get(4)?;
                let pid: Option<u32> = row.get(5)?;
                let state_str: String = row.get(6)?;
                let expected_json: String = row.get(7)?;
                let baseline_time_str: Option<String> = row.get(8)?;
                let verif_json: Option<String> = row.get(9)?;
                let baseline_json: Option<String> = row.get(10)?;
                let mode_str: Option<String> = row.get(11)?;

                let parsed_sid = LaunchSessionId::from_str(&sid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let game_installation_id = GameInstallationId::from_str(&gid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let launched_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let ended_at = ended_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                });
                let expected_mod_ids: Vec<ModUniqueId> = serde_json::from_str(&expected_json)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            7,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let log_baseline_time = baseline_time_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                });
                let verification_result: Option<VerificationResult> =
                    verif_json.and_then(|v| serde_json::from_str(&v).ok());
                let log_baseline = baseline_json.and_then(|v| serde_json::from_str(&v).ok());

                let state = match state_str.as_str() {
                    "running_unverified" => SessionState::RunningUnverified,
                    "mod_load_confirmed" => SessionState::ModLoadConfirmed,
                    "exited" => SessionState::Exited,
                    "failed" => SessionState::Failed,
                    "verification_unavailable" => SessionState::VerificationUnavailable,
                    _ => SessionState::Starting,
                };

                let launch_mode = match mode_str.as_deref() {
                    Some("vanilla") => LaunchMode::Vanilla,
                    Some("runtime_test") => LaunchMode::RuntimeTest,
                    _ => LaunchMode::Modded,
                };

                Ok(LaunchSession {
                    id: parsed_sid,
                    game_installation_id,
                    profile_id,
                    launch_mode,
                    launched_at,
                    ended_at,
                    pid,
                    state,
                    expected_mod_ids,
                    log_baseline_time,
                    log_baseline,
                    verification_result,
                })
            })
            .optional()
            .map_err(map_db_err)?
        } else {
            stmt.query_row([], |row| {
                let sid_str: String = row.get(0)?;
                let gid_str: String = row.get(1)?;
                let pid_str: String = row.get(2)?;
                let date_str: String = row.get(3)?;
                let ended_str: Option<String> = row.get(4)?;
                let pid: Option<u32> = row.get(5)?;
                let state_str: String = row.get(6)?;
                let expected_json: String = row.get(7)?;
                let baseline_time_str: Option<String> = row.get(8)?;
                let verif_json: Option<String> = row.get(9)?;
                let baseline_json: Option<String> = row.get(10)?;
                let mode_str: Option<String> = row.get(11)?;

                let parsed_sid = LaunchSessionId::from_str(&sid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let game_installation_id = GameInstallationId::from_str(&gid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let launched_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let ended_at = ended_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                });
                let expected_mod_ids: Vec<ModUniqueId> = serde_json::from_str(&expected_json)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            7,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let log_baseline_time = baseline_time_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                });
                let verification_result: Option<VerificationResult> =
                    verif_json.and_then(|v| serde_json::from_str(&v).ok());
                let log_baseline = baseline_json.and_then(|v| serde_json::from_str(&v).ok());

                let state = match state_str.as_str() {
                    "running_unverified" => SessionState::RunningUnverified,
                    "mod_load_confirmed" => SessionState::ModLoadConfirmed,
                    "exited" => SessionState::Exited,
                    "failed" => SessionState::Failed,
                    "verification_unavailable" => SessionState::VerificationUnavailable,
                    _ => SessionState::Starting,
                };

                let launch_mode = match mode_str.as_deref() {
                    Some("vanilla") => LaunchMode::Vanilla,
                    Some("runtime_test") => LaunchMode::RuntimeTest,
                    _ => LaunchMode::Modded,
                };

                Ok(LaunchSession {
                    id: parsed_sid,
                    game_installation_id,
                    profile_id,
                    launch_mode,
                    launched_at,
                    ended_at,
                    pid,
                    state,
                    expected_mod_ids,
                    log_baseline_time,
                    log_baseline,
                    verification_result,
                })
            })
            .optional()
            .map_err(map_db_err)?
        };

        Ok(session)
    }

    fn update_launch_session(&self, session: &LaunchSession) -> AppResult<()> {
        LaunchSessionRepository::save_launch_session(self, session)
    }

    fn list_launch_sessions(
        &self,
        profile_id: &ProfileId,
        limit: usize,
    ) -> AppResult<Vec<LaunchSession>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, game_id, setup_id, launched_at, ended_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json, launch_mode
                 FROM launch_sessions WHERE setup_id = ?1 ORDER BY launched_at DESC LIMIT ?2",
            )
            .map_err(map_db_err)?;

        let rows = stmt
            .query_map(params![profile_id.to_string(), limit as i64], |row| {
                let sid_str: String = row.get(0)?;
                let gid_str: String = row.get(1)?;
                let pid_str: String = row.get(2)?;
                let date_str: String = row.get(3)?;
                let ended_str: Option<String> = row.get(4)?;
                let pid: Option<u32> = row.get(5)?;
                let state_str: String = row.get(6)?;
                let expected_json: String = row.get(7)?;
                let baseline_time_str: Option<String> = row.get(8)?;
                let verif_json: Option<String> = row.get(9)?;
                let baseline_json: Option<String> = row.get(10)?;
                let mode_str: Option<String> = row.get(11)?;

                let parsed_sid = LaunchSessionId::from_str(&sid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let game_installation_id = GameInstallationId::from_str(&gid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let profile_id = ProfileId::from_str(&pid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let launched_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let ended_at = ended_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                });
                let expected_mod_ids: Vec<ModUniqueId> = serde_json::from_str(&expected_json)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            7,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let log_baseline_time = baseline_time_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|d| d.with_timezone(&Utc))
                });
                let verification_result: Option<VerificationResult> =
                    verif_json.and_then(|v| serde_json::from_str(&v).ok());
                let log_baseline = baseline_json.and_then(|v| serde_json::from_str(&v).ok());

                let state = match state_str.as_str() {
                    "running_unverified" => SessionState::RunningUnverified,
                    "mod_load_confirmed" => SessionState::ModLoadConfirmed,
                    "exited" => SessionState::Exited,
                    "failed" => SessionState::Failed,
                    "verification_unavailable" => SessionState::VerificationUnavailable,
                    _ => SessionState::Starting,
                };

                let launch_mode = match mode_str.as_deref() {
                    Some("vanilla") => LaunchMode::Vanilla,
                    Some("runtime_test") => LaunchMode::RuntimeTest,
                    _ => LaunchMode::Modded,
                };

                Ok(LaunchSession {
                    id: parsed_sid,
                    game_installation_id,
                    profile_id,
                    launch_mode,
                    launched_at,
                    ended_at,
                    pid,
                    state,
                    expected_mod_ids,
                    log_baseline_time,
                    log_baseline,
                    verification_result,
                })
            })
            .map_err(map_db_err)?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(map_db_err)?);
        }
        Ok(list)
    }
}

// ---------------------------------------------------------------------------
// SmapiRepository
// ---------------------------------------------------------------------------
impl SmapiRepository for SqliteStateRepository {
    fn save_smapi_installation(&self, record: &ManagedSmapiInstallation) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;

        conn.execute(
            "INSERT INTO smapi_installations (id, game_id, release_version, adapter_version, release_policy_id, installed_at)
             VALUES (?1, ?2, ?3, '1.0.0', ?4, ?5)
             ON CONFLICT(game_id) DO UPDATE SET
                release_version=excluded.release_version,
                release_policy_id=excluded.release_policy_id,
                installed_at=excluded.installed_at",
            params![
                record.game_installation_id.to_string(),
                record.game_installation_id.to_string(),
                record.release_version,
                record.release_policy_id,
                record.installed_at.to_rfc3339(),
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_smapi_installation(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Option<ManagedSmapiInstallation>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, game_id, release_version, release_policy_id, installed_at
                 FROM smapi_installations WHERE game_id = ?1",
            )
            .map_err(map_db_err)?;

        let record = stmt
            .query_row(params![game_id.to_string()], |row| {
                let _id: String = row.get(0)?;
                let gid_str: String = row.get(1)?;
                let release_version: String = row.get(2)?;
                let release_policy_id: Option<String> = row.get(3)?;
                let date_str: String = row.get(4)?;

                let parsed_gid = GameInstallationId::from_str(&gid_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let installed_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(ManagedSmapiInstallation {
                    game_installation_id: parsed_gid,
                    release_version,
                    release_policy_id: release_policy_id.unwrap_or_else(|| "default".to_string()),
                    installed_at,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(record)
    }
}

// ---------------------------------------------------------------------------
// PreferencesRepository
// ---------------------------------------------------------------------------
impl PreferencesRepository for SqliteStateRepository {
    fn get_window_geometry(&self) -> AppResult<Option<WindowGeometryDto>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare("SELECT width, height, x, y, is_maximized FROM window_geometry WHERE id = 1")
            .map_err(map_db_err)?;

        let geom = stmt
            .query_row([], |row| {
                let width: u32 = row.get(0)?;
                let height: u32 = row.get(1)?;
                let x: i32 = row.get(2)?;
                let y: i32 = row.get(3)?;
                let max_int: i32 = row.get(4)?;

                Ok(WindowGeometryDto {
                    width,
                    height,
                    x,
                    y,
                    is_maximized: max_int != 0,
                })
            })
            .optional()
            .map_err(map_db_err)?;

        Ok(geom)
    }

    fn save_window_geometry(&self, geom: &WindowGeometryDto) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        conn.execute(
            "INSERT INTO window_geometry (id, schema_version, width, height, x, y, is_maximized)
             VALUES (1, 1, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                width=excluded.width,
                height=excluded.height,
                x=excluded.x,
                y=excluded.y,
                is_maximized=excluded.is_maximized",
            params![
                geom.width,
                geom.height,
                geom.x,
                geom.y,
                if geom.is_maximized { 1 } else { 0 },
            ],
        )
        .map_err(map_db_err)?;
        Ok(())
    }

    fn get_preference(&self, key: &str) -> AppResult<Option<String>> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        let mut stmt = conn
            .prepare("SELECT details_json FROM operation_steps WHERE step_kind = ?1 LIMIT 1")
            .map_err(map_db_err)?;

        let val = stmt
            .query_row(params![format!("pref_{}", key)], |row| row.get(0))
            .optional()
            .map_err(map_db_err)?;

        Ok(val)
    }

    fn set_preference(&self, key: &str, value: &str) -> AppResult<()> {
        let conn = self.conn.lock().map_err(map_db_err)?;
        conn.execute(
            "INSERT INTO operation_steps (operation_id, step_index, step_kind, state, payload_json, error_json)
             VALUES ('00000000-0000-0000-0000-000000000000', 0, ?1, 'completed', ?2, NULL)
             ON CONFLICT(operation_id, step_index) DO UPDATE SET payload_json=excluded.payload_json",
            params![format!("pref_{}", key), value],
        )
        .map_err(map_db_err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AtomicMutationStore
// ---------------------------------------------------------------------------
impl AtomicMutationStore for SqliteStateRepository {
    fn commit_install(&self, commit: InstallCommit) -> AppResult<()> {
        let mut conn = self.conn.lock().map_err(map_db_err)?;
        let tx = conn.transaction().map_err(map_db_err)?;

        // 1. Verify profile revision matches
        let current_revision: u64 = tx
            .query_row(
                "SELECT revision FROM profiles WHERE id = ?1",
                params![commit.profile_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|e| {
                AppError::conflict(
                    "PROFILE_NOT_FOUND",
                    format!("Profile '{}' not found: {}", commit.profile_id, e),
                )
            })?;

        if current_revision != commit.expected_profile_revision {
            return Err(AppError::conflict(
                "PROFILE_REVISION_MISMATCH",
                format!(
                    "Profile revision changed from {} to {} during preview",
                    commit.expected_profile_revision, current_revision
                ),
            ));
        }

        // 2. Increment revision
        let new_revision = current_revision + 1;
        tx.execute(
            "UPDATE profiles SET revision = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?2",
            params![new_revision, commit.profile_id.to_string()],
        )
        .map_err(map_db_err)?;

        // 3. Insert artifact
        tx.execute(
            "INSERT INTO packages (hash, byte_size, created_at, original_filename, source_kind, storage_relative_path, first_seen_at)
             VALUES (?1, ?2, ?3, ?4, 'artifact', ?5, ?6)
             ON CONFLICT(hash) DO NOTHING",
            params![
                commit.artifact.hash.as_str(),
                commit.artifact.byte_size,
                commit.artifact.first_seen_at.to_rfc3339(),
                commit.acquisition.original_filename,
                commit.artifact.storage_relative_path,
                commit.artifact.first_seen_at.to_rfc3339(),
            ],
        )
        .map_err(map_db_err)?;

        // 4. Insert acquisition
        let src_str = match commit.acquisition.source {
            AcquisitionSource::LocalFile => "local_file",
            AcquisitionSource::DirectUrl => "direct_url",
            AcquisitionSource::Provider => "provider",
            AcquisitionSource::ManualReference => "manual_reference",
        };
        tx.execute(
            "INSERT INTO acquisitions (id, artifact_hash, source, original_filename, acquired_at, expected_hash, source_metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO NOTHING",
            params![
                commit.acquisition.id.to_string(),
                commit.acquisition.artifact_hash.as_str(),
                src_str,
                commit.acquisition.original_filename,
                commit.acquisition.acquired_at.to_rfc3339(),
                commit.acquisition.expected_hash.as_ref().map(|h| h.as_str()),
                commit.acquisition.source_metadata,
            ],
        )
        .map_err(map_db_err)?;

        // 5. Insert package components
        for comp in &commit.package_components {
            let manifest_json = serde_json::to_string(&comp.manifest)
                .map_err(|e| AppError::internal("Serialize manifest error", e.to_string()))?;
            tx.execute(
                "INSERT INTO package_components (id, artifact_hash, unique_id, name, author, version, description, relative_component_root, raw_manifest, manifest_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO NOTHING",
                params![
                    comp.id.to_string(),
                    comp.artifact_hash.as_str(),
                    comp.unique_id.as_str(),
                    comp.name,
                    comp.author,
                    comp.version,
                    comp.description,
                    comp.relative_component_root,
                    comp.raw_manifest,
                    manifest_json,
                ],
            )
            .map_err(map_db_err)?;
        }

        // 6. Insert profile deployment
        let state_str = match commit.deployment.state {
            DeploymentState::Present => "present",
            DeploymentState::Disabled => "disabled",
            DeploymentState::Missing => "missing",
            DeploymentState::ExternallyModified => "externally_modified",
            DeploymentState::Quarantined => "quarantined",
        };
        tx.execute(
            "INSERT INTO profile_deployments (id, profile_id, artifact_hash, root_relative_path, installed_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET state=excluded.state, root_relative_path=excluded.root_relative_path",
            params![
                commit.deployment.id.to_string(),
                commit.deployment.profile_id.to_string(),
                commit.deployment.artifact_hash.as_str(),
                commit.deployment.root_relative_path,
                commit.deployment.installed_at.to_rfc3339(),
                state_str,
            ],
        )
        .map_err(map_db_err)?;

        // 7. Insert profile components
        for pc in &commit.profile_components {
            let reason_str = match pc.installed_reason {
                InstalledReason::Direct => "direct",
                InstalledReason::Dependency => "dependency",
                InstalledReason::BundleCompanion => "bundle_companion",
            };
            tx.execute(
                "INSERT INTO profile_components (id, profile_id, deployment_id, package_component_id, enabled, installed_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET enabled=excluded.enabled, installed_reason=excluded.installed_reason",
                params![
                    pc.id.to_string(),
                    pc.profile_id.to_string(),
                    pc.deployment_id.to_string(),
                    pc.package_component_id.to_string(),
                    if pc.enabled { 1 } else { 0 },
                    reason_str,
                ],
            )
            .map_err(map_db_err)?;
        }

        // 8. Insert effects
        for effect in &commit.effects {
            tx.execute(
                "INSERT INTO operation_effects (id, operation_id, profile_id, entity_type, entity_id, change_kind, before_json, after_json, occurred_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO NOTHING",
                params![
                    effect.id,
                    effect.operation_id.to_string(),
                    effect.profile_id.as_ref().map(|id| id.to_string()),
                    effect.entity_type,
                    effect.entity_id,
                    effect.change_kind,
                    effect.before_json,
                    effect.after_json,
                    effect.occurred_at.to_rfc3339(),
                ],
            )
            .map_err(map_db_err)?;
        }

        // 9. Mark operation succeeded
        tx.execute(
            "UPDATE operations SET state = 'succeeded', completed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
            params![commit.operation_id.to_string()],
        )
        .map_err(map_db_err)?;

        tx.commit().map_err(map_db_err)?;
        Ok(())
    }

    fn commit_removal(&self, commit: RemovalCommit) -> AppResult<()> {
        let mut conn = self.conn.lock().map_err(map_db_err)?;
        let tx = conn.transaction().map_err(map_db_err)?;

        // 1. Verify profile revision matches
        let current_revision: u64 = tx
            .query_row(
                "SELECT revision FROM profiles WHERE id = ?1",
                params![commit.profile_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|e| {
                AppError::conflict(
                    "PROFILE_NOT_FOUND",
                    format!("Profile '{}' not found: {}", commit.profile_id, e),
                )
            })?;

        if current_revision != commit.expected_profile_revision {
            return Err(AppError::conflict(
                "PROFILE_REVISION_MISMATCH",
                format!(
                    "Profile revision changed from {} to {} during preview",
                    commit.expected_profile_revision, current_revision
                ),
            ));
        }

        // 2. Increment revision
        let new_revision = current_revision + 1;
        tx.execute(
            "UPDATE profiles SET revision = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?2",
            params![new_revision, commit.profile_id.to_string()],
        )
        .map_err(map_db_err)?;

        // 3. Delete profile components
        for cid in &commit.removed_profile_component_ids {
            tx.execute(
                "DELETE FROM profile_components WHERE id = ?1",
                params![cid.to_string()],
            )
            .map_err(map_db_err)?;
        }

        // 4. Mark deployment quarantined/removed
        tx.execute(
            "UPDATE profile_deployments SET state = 'quarantined' WHERE id = ?1",
            params![commit.deployment_id.to_string()],
        )
        .map_err(map_db_err)?;

        // 5. Insert effects
        for effect in &commit.effects {
            tx.execute(
                "INSERT INTO operation_effects (id, operation_id, profile_id, entity_type, entity_id, change_kind, before_json, after_json, occurred_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO NOTHING",
                params![
                    effect.id,
                    effect.operation_id.to_string(),
                    effect.profile_id.as_ref().map(|id| id.to_string()),
                    effect.entity_type,
                    effect.entity_id,
                    effect.change_kind,
                    effect.before_json,
                    effect.after_json,
                    effect.occurred_at.to_rfc3339(),
                ],
            )
            .map_err(map_db_err)?;
        }

        // 6. Mark operation succeeded
        tx.execute(
            "UPDATE operations SET state = 'succeeded', completed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
            params![commit.operation_id.to_string()],
        )
        .map_err(map_db_err)?;

        tx.commit().map_err(map_db_err)?;
        Ok(())
    }

    fn commit_profile_create(&self, commit: ProfileCreateCommit) -> AppResult<()> {
        let mut conn = self.conn.lock().map_err(map_db_err)?;
        let tx = conn.transaction().map_err(map_db_err)?;

        let state_str = match commit.profile.state {
            ProfileState::Active => "active",
            ProfileState::Archived => "archived",
            ProfileState::Corrupted => "corrupted",
        };

        tx.execute(
            "INSERT INTO profiles (id, game_installation_id, name, description, revision, created_at, updated_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                commit.profile.id.to_string(),
                commit.profile.game_installation_id.to_string(),
                commit.profile.name,
                commit.profile.description,
                commit.profile.revision,
                commit.profile.created_at.to_rfc3339(),
                commit.profile.updated_at.to_rfc3339(),
                state_str,
            ],
        )
        .map_err(map_db_err)?;

        if commit.set_as_default {
            tx.execute(
                "INSERT INTO game_profile_context (game_installation_id, active_profile_id, default_profile_id, last_active_profile_id, updated_at)
                 VALUES (?1, ?2, ?2, ?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 ON CONFLICT(game_installation_id) DO UPDATE SET
                    default_profile_id=excluded.default_profile_id,
                    updated_at=excluded.updated_at",
                params![
                    commit.profile.game_installation_id.to_string(),
                    commit.profile.id.to_string(),
                ],
            )
            .map_err(map_db_err)?;
        }

        if commit.set_as_active {
            tx.execute(
                "UPDATE app_context SET active_game_installation_id = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = 1",
                params![commit.profile.game_installation_id.to_string()],
            )
            .map_err(map_db_err)?;

            tx.execute(
                "INSERT INTO game_profile_context (game_installation_id, active_profile_id, default_profile_id, last_active_profile_id, updated_at)
                 VALUES (?1, ?2, ?2, ?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 ON CONFLICT(game_installation_id) DO UPDATE SET
                    active_profile_id=excluded.active_profile_id,
                    last_active_profile_id=excluded.last_active_profile_id,
                    updated_at=excluded.updated_at",
                params![
                    commit.profile.game_installation_id.to_string(),
                    commit.profile.id.to_string(),
                ],
            )
            .map_err(map_db_err)?;
        }

        for effect in &commit.effects {
            tx.execute(
                "INSERT INTO operation_effects (id, operation_id, profile_id, entity_type, entity_id, change_kind, before_json, after_json, occurred_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO NOTHING",
                params![
                    effect.id,
                    effect.operation_id.to_string(),
                    effect.profile_id.as_ref().map(|id| id.to_string()),
                    effect.entity_type,
                    effect.entity_id,
                    effect.change_kind,
                    effect.before_json,
                    effect.after_json,
                    effect.occurred_at.to_rfc3339(),
                ],
            )
            .map_err(map_db_err)?;
        }

        tx.commit().map_err(map_db_err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Legacy StateRepository compatibility implementation
// ---------------------------------------------------------------------------
use manager_core::domain::{
    GameInstallation as LegacyGame, InstalledMod, LaunchSession as LegacySession,
    Operation as LegacyOp, OperationKind as LegacyOpKind, OperationState as LegacyOpState,
    Package as LegacyPkg, Setup, SmapiInstallationRecord, StoreKind, WindowGeometry,
};
use manager_core::ports::StateRepository;

impl StateRepository for SqliteStateRepository {
    fn save_game(&self, game: &LegacyGame) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let kind_str = match game.platform_kind {
            StoreKind::SteamNative => "steam_native",
            StoreKind::ManualFolder => "manual_folder",
            StoreKind::Unsupported => "unsupported",
        };
        conn.execute(
            "INSERT INTO game_installations (id, canonical_root, platform_kind, detected_version, validated_at, is_fresh, validation_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                canonical_root=excluded.canonical_root,
                platform_kind=excluded.platform_kind,
                detected_version=excluded.detected_version,
                validated_at=excluded.validated_at,
                is_fresh=excluded.is_fresh,
                validation_error=excluded.validation_error",
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
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_game(&self, id: &str) -> Result<Option<LegacyGame>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, canonical_root, platform_kind, detected_version, validated_at, is_fresh, validation_error FROM game_installations WHERE id = ?1")
            .map_err(|e| e.to_string())?;

        let game = stmt
            .query_row(params![id], |row| {
                let id: String = row.get(0)?;
                let root: String = row.get(1)?;
                let kind_str: String = row.get(2)?;
                let ver: Option<String> = row.get(3)?;
                let val_at: String = row.get(4)?;
                let fresh: i32 = row.get(5)?;
                let err: Option<String> = row.get(6)?;

                let platform_kind = match kind_str.as_str() {
                    "steam_native" => StoreKind::SteamNative,
                    "manual_folder" => StoreKind::ManualFolder,
                    _ => StoreKind::Unsupported,
                };
                let validated_at = DateTime::parse_from_rfc3339(&val_at)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(LegacyGame {
                    id,
                    canonical_root: PathBuf::from(root),
                    platform_kind,
                    detected_version: ver,
                    validated_at,
                    is_fresh: fresh != 0,
                    is_managed: false,
                    validation_error: err,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(game)
    }

    fn list_games(&self) -> Result<Vec<LegacyGame>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, canonical_root, platform_kind, detected_version, validated_at, is_fresh, validation_error FROM game_installations")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let root: String = row.get(1)?;
                let kind_str: String = row.get(2)?;
                let ver: Option<String> = row.get(3)?;
                let val_at: String = row.get(4)?;
                let fresh: i32 = row.get(5)?;
                let err: Option<String> = row.get(6)?;

                let platform_kind = match kind_str.as_str() {
                    "steam_native" => StoreKind::SteamNative,
                    "manual_folder" => StoreKind::ManualFolder,
                    _ => StoreKind::Unsupported,
                };
                let validated_at = DateTime::parse_from_rfc3339(&val_at)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(LegacyGame {
                    id,
                    canonical_root: PathBuf::from(root),
                    platform_kind,
                    detected_version: ver,
                    validated_at,
                    is_fresh: fresh != 0,
                    is_managed: false,
                    validation_error: err,
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
            "INSERT INTO setups (id, game_id, display_name, relative_mods_dir, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name, relative_mods_dir=excluded.relative_mods_dir",
            params![
                setup.id,
                setup.game_id,
                setup.display_name,
                setup.relative_mods_dir,
                setup.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_setup(&self, id: &str) -> Result<Option<Setup>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, game_id, display_name, relative_mods_dir, created_at FROM setups WHERE id = ?1")
            .map_err(|e| e.to_string())?;

        let setup = stmt
            .query_row(params![id], |row| {
                let id: String = row.get(0)?;
                let game_id: String = row.get(1)?;
                let display_name: String = row.get(2)?;
                let relative_mods_dir: String = row.get(3)?;
                let date_str: String = row.get(4)?;
                let created_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(Setup {
                    id,
                    game_id,
                    display_name,
                    relative_mods_dir,
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
            .prepare("SELECT id, game_id, display_name, relative_mods_dir, created_at FROM setups WHERE game_id = ?1 ORDER BY created_at ASC LIMIT 1")
            .map_err(|e| e.to_string())?;

        let setup = stmt
            .query_row(params![game_id], |row| {
                let id: String = row.get(0)?;
                let game_id: String = row.get(1)?;
                let display_name: String = row.get(2)?;
                let relative_mods_dir: String = row.get(3)?;
                let date_str: String = row.get(4)?;
                let created_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(Setup {
                    id,
                    game_id,
                    display_name,
                    relative_mods_dir,
                    created_at,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(setup)
    }

    fn save_package(&self, pkg: &LegacyPkg) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO packages (hash, original_filename, source_kind, byte_size, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(hash) DO NOTHING",
            params![
                pkg.hash,
                pkg.original_filename,
                pkg.source_kind,
                pkg.byte_size,
                pkg.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_package(&self, hash: &str) -> Result<Option<LegacyPkg>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT hash, original_filename, source_kind, byte_size, created_at FROM packages WHERE hash = ?1")
            .map_err(|e| e.to_string())?;

        let pkg = stmt
            .query_row(params![hash], |row| {
                let hash: String = row.get(0)?;
                let original_filename: String = row.get(1)?;
                let source_kind: String = row.get(2)?;
                let byte_size: u64 = row.get(3)?;
                let date_str: String = row.get(4)?;
                let created_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(LegacyPkg {
                    hash,
                    original_filename,
                    source_kind,
                    byte_size,
                    created_at,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(pkg)
    }

    fn save_installed_mod(&self, mod_item: &InstalledMod) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let inv_json = serde_json::to_string(&mod_item.file_inventory).unwrap_or_default();
        conn.execute(
            "INSERT INTO installed_mods (id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO NOTHING",
            params![
                mod_item.id,
                mod_item.setup_id,
                mod_item.package_id,
                mod_item.unique_id,
                mod_item.name,
                mod_item.author,
                mod_item.version,
                mod_item.description,
                mod_item.raw_manifest,
                mod_item.relative_target_path,
                inv_json,
                mod_item.installed_at.to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_installed_mod(&self, id: &str) -> Result<Option<InstalledMod>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at FROM installed_mods WHERE id = ?1")
            .map_err(|e| e.to_string())?;

        let m = stmt
            .query_row(params![id], |row| {
                let id: String = row.get(0)?;
                let setup_id: String = row.get(1)?;
                let package_id: String = row.get(2)?;
                let unique_id: String = row.get(3)?;
                let name: String = row.get(4)?;
                let author: String = row.get(5)?;
                let version: String = row.get(6)?;
                let description: Option<String> = row.get(7)?;
                let raw_manifest: String = row.get(8)?;
                let relative_target_path: String = row.get(9)?;
                let inv_str: String = row.get(10)?;
                let date_str: String = row.get(11)?;

                let installed_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let file_inventory: Vec<String> =
                    serde_json::from_str(&inv_str).unwrap_or_default();

                Ok(InstalledMod {
                    id,
                    setup_id,
                    package_id,
                    unique_id,
                    name,
                    author,
                    version,
                    description,
                    raw_manifest,
                    relative_target_path,
                    file_inventory,
                    installed_at,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(m)
    }

    fn list_installed_mods(&self, setup_id: &str) -> Result<Vec<InstalledMod>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at FROM installed_mods WHERE setup_id = ?1 ORDER BY installed_at ASC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![setup_id], |row| {
                let id: String = row.get(0)?;
                let setup_id: String = row.get(1)?;
                let package_id: String = row.get(2)?;
                let unique_id: String = row.get(3)?;
                let name: String = row.get(4)?;
                let author: String = row.get(5)?;
                let version: String = row.get(6)?;
                let description: Option<String> = row.get(7)?;
                let raw_manifest: String = row.get(8)?;
                let relative_target_path: String = row.get(9)?;
                let inv_str: String = row.get(10)?;
                let date_str: String = row.get(11)?;

                let installed_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let file_inventory: Vec<String> =
                    serde_json::from_str(&inv_str).unwrap_or_default();

                Ok(InstalledMod {
                    id,
                    setup_id,
                    package_id,
                    unique_id,
                    name,
                    author,
                    version,
                    description,
                    raw_manifest,
                    relative_target_path,
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
        conn.execute("DELETE FROM installed_mods WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn save_operation(&self, op: &LegacyOp) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let kind_str = match op.kind {
            LegacyOpKind::SmapiSetup => "smapi_setup",
            LegacyOpKind::ModInstall => "mod_install",
            LegacyOpKind::ModRemove => "mod_remove",
            LegacyOpKind::GameLaunch => "game_launch",
        };
        let state_str = match op.state {
            LegacyOpState::Pending => "pending",
            LegacyOpState::Prepared => "prepared",
            LegacyOpState::Running => "running",
            LegacyOpState::Completed => "succeeded",
            LegacyOpState::Failed => "failed",
            LegacyOpState::Recovering => "recovery_required",
        };

        conn.execute(
            "INSERT INTO operations (id, kind, state, plan_json, error_json, created_at, updated_at, schema_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET state=excluded.state, error_json=excluded.error_json, updated_at=excluded.updated_at",
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
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_operation(&self, id: &str) -> Result<Option<LegacyOp>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, kind, state, plan_json, error_json, created_at, updated_at, schema_version FROM operations WHERE id = ?1")
            .map_err(|e| e.to_string())?;

        let op = stmt
            .query_row(params![id], |row| {
                let id: String = row.get(0)?;
                let kind_str: String = row.get(1)?;
                let state_str: String = row.get(2)?;
                let plan_json: String = row.get(3)?;
                let error_json: Option<String> = row.get(4)?;
                let created_str: String = row.get(5)?;
                let updated_str: String = row.get(6)?;
                let schema_version: u32 = row.get(7)?;

                let created_at = parse_db_datetime(&created_str, 5)?;
                let updated_at = parse_db_datetime(&updated_str, 6)?;

                let kind = match kind_str.as_str() {
                    "mod_install" => LegacyOpKind::ModInstall,
                    "mod_remove" => LegacyOpKind::ModRemove,
                    "game_launch" => LegacyOpKind::GameLaunch,
                    _ => LegacyOpKind::SmapiSetup,
                };
                let state = match state_str.as_str() {
                    "pending" => LegacyOpState::Pending,
                    "prepared" => LegacyOpState::Prepared,
                    "running" => LegacyOpState::Running,
                    "succeeded" | "completed" => LegacyOpState::Completed,
                    "recovery_required" | "recovering" => LegacyOpState::Recovering,
                    _ => LegacyOpState::Failed,
                };

                Ok(LegacyOp {
                    id,
                    kind,
                    state,
                    plan_json,
                    error_json,
                    created_at,
                    updated_at,
                    schema_version,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(op)
    }

    fn update_operation_state(
        &self,
        id: &str,
        state: LegacyOpState,
        error_json: Option<String>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let state_str = match state {
            LegacyOpState::Pending => "pending",
            LegacyOpState::Prepared => "prepared",
            LegacyOpState::Running => "running",
            LegacyOpState::Completed => "succeeded",
            LegacyOpState::Failed => "failed",
            LegacyOpState::Recovering => "recovery_required",
        };
        conn.execute(
            "UPDATE operations SET state = ?1, error_json = COALESCE(?2, error_json), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?3",
            params![state_str, error_json, id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn list_unresolved_operations(&self) -> Result<Vec<LegacyOp>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, kind, state, plan_json, error_json, created_at, updated_at, schema_version FROM operations WHERE state NOT IN ('succeeded', 'completed', 'cancelled') ORDER BY created_at ASC")
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let kind_str: String = row.get(1)?;
                let state_str: String = row.get(2)?;
                let plan_json: String = row.get(3)?;
                let error_json: Option<String> = row.get(4)?;
                let created_str: String = row.get(5)?;
                let updated_str: String = row.get(6)?;
                let schema_version: u32 = row.get(7)?;

                let created_at = parse_db_datetime(&created_str, 5)?;
                let updated_at = parse_db_datetime(&updated_str, 6)?;

                let kind = match kind_str.as_str() {
                    "mod_install" => LegacyOpKind::ModInstall,
                    "mod_remove" => LegacyOpKind::ModRemove,
                    "game_launch" => LegacyOpKind::GameLaunch,
                    _ => LegacyOpKind::SmapiSetup,
                };
                let state = match state_str.as_str() {
                    "pending" => LegacyOpState::Pending,
                    "prepared" => LegacyOpState::Prepared,
                    "running" => LegacyOpState::Running,
                    "succeeded" | "completed" => LegacyOpState::Completed,
                    "recovery_required" | "recovering" => LegacyOpState::Recovering,
                    _ => LegacyOpState::Failed,
                };

                Ok(LegacyOp {
                    id,
                    kind,
                    state,
                    plan_json,
                    error_json,
                    created_at,
                    updated_at,
                    schema_version,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r.map_err(|e| e.to_string())?);
        }
        Ok(list)
    }

    fn save_launch_session(&self, session: &LegacySession) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let state_str = match session.state {
            manager_core::domain::SessionState::Starting => "starting",
            manager_core::domain::SessionState::RunningUnverified => "running_unverified",
            manager_core::domain::SessionState::ModLoadConfirmed => "mod_load_confirmed",
            manager_core::domain::SessionState::Exited => "exited",
            manager_core::domain::SessionState::Failed => "failed",
            manager_core::domain::SessionState::VerificationUnavailable => {
                "verification_unavailable"
            }
        };
        let expected_json = serde_json::to_string(&session.expected_mod_ids).unwrap_or_default();
        let verif_json = session
            .verification_result
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok());
        let baseline_json = session
            .log_baseline
            .as_ref()
            .and_then(|b| serde_json::to_string(b).ok());

        conn.execute(
            "INSERT INTO launch_sessions (id, game_id, setup_id, launched_at, pid, state, expected_mod_ids_json, log_baseline_time, verification_result_json, log_baseline_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET pid=excluded.pid, state=excluded.state, verification_result_json=excluded.verification_result_json, log_baseline_json=COALESCE(excluded.log_baseline_json, launch_sessions.log_baseline_json)",
            params![
                session.id,
                session.game_id,
                session.setup_id,
                session.launched_at.to_rfc3339(),
                session.pid,
                state_str,
                expected_json,
                session.launched_at.to_rfc3339(),
                verif_json,
                baseline_json,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_launch_session(&self, id: &str) -> Result<Option<LegacySession>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, game_id, setup_id, launched_at, pid, state, expected_mod_ids_json, verification_result_json, log_baseline_json FROM launch_sessions WHERE id = ?1")
            .map_err(|e| e.to_string())?;

        let session = stmt
            .query_row(params![id], |row| {
                let id: String = row.get(0)?;
                let game_id: String = row.get(1)?;
                let setup_id: String = row.get(2)?;
                let date_str: String = row.get(3)?;
                let pid: Option<u32> = row.get(4)?;
                let state_str: String = row.get(5)?;
                let expected_json: String = row.get(6)?;
                let verif_json: Option<String> = row.get(7)?;
                let baseline_json: Option<String> = row.get(8)?;

                let launched_at = parse_db_datetime(&date_str, 3)?;
                let expected_mod_ids: Vec<String> =
                    serde_json::from_str(&expected_json).unwrap_or_default();
                let verification_result: Option<manager_core::domain::VerificationResult> =
                    verif_json.and_then(|v| serde_json::from_str(&v).ok());
                let log_baseline: Option<manager_core::launch::SessionVerificationBaseline> =
                    baseline_json.and_then(|b| serde_json::from_str(&b).ok());

                let state = match state_str.as_str() {
                    "running_unverified" => manager_core::domain::SessionState::RunningUnverified,
                    "mod_load_confirmed" => manager_core::domain::SessionState::ModLoadConfirmed,
                    "exited" => manager_core::domain::SessionState::Exited,
                    "failed" => manager_core::domain::SessionState::Failed,
                    "verification_unavailable" => {
                        manager_core::domain::SessionState::VerificationUnavailable
                    }
                    _ => manager_core::domain::SessionState::Starting,
                };

                Ok(LegacySession {
                    id,
                    game_id,
                    setup_id,
                    launched_at,
                    pid,
                    state,
                    expected_mod_ids,
                    log_baseline_time: None,
                    log_baseline,
                    verification_result,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(session)
    }

    fn get_latest_launch_session(
        &self,
        game_id: Option<&str>,
    ) -> Result<Option<LegacySession>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let query = if game_id.is_some() {
            "SELECT id, game_id, setup_id, launched_at, pid, state, expected_mod_ids_json, verification_result_json, log_baseline_json FROM launch_sessions WHERE game_id = ?1 ORDER BY launched_at DESC LIMIT 1"
        } else {
            "SELECT id, game_id, setup_id, launched_at, pid, state, expected_mod_ids_json, verification_result_json, log_baseline_json FROM launch_sessions ORDER BY launched_at DESC LIMIT 1"
        };
        let mut stmt = conn.prepare(query).map_err(|e| e.to_string())?;

        let session = if let Some(gid) = game_id {
            stmt.query_row(params![gid], |row| {
                let id: String = row.get(0)?;
                let game_id: String = row.get(1)?;
                let setup_id: String = row.get(2)?;
                let date_str: String = row.get(3)?;
                let pid: Option<u32> = row.get(4)?;
                let state_str: String = row.get(5)?;
                let expected_json: String = row.get(6)?;
                let verif_json: Option<String> = row.get(7)?;
                let baseline_json: Option<String> = row.get(8)?;

                let launched_at = parse_db_datetime(&date_str, 3)?;
                let expected_mod_ids: Vec<String> =
                    serde_json::from_str(&expected_json).unwrap_or_default();
                let verification_result: Option<manager_core::domain::VerificationResult> =
                    verif_json.and_then(|v| serde_json::from_str(&v).ok());
                let log_baseline: Option<manager_core::launch::SessionVerificationBaseline> =
                    baseline_json.and_then(|b| serde_json::from_str(&b).ok());

                let state = match state_str.as_str() {
                    "running_unverified" => manager_core::domain::SessionState::RunningUnverified,
                    "mod_load_confirmed" => manager_core::domain::SessionState::ModLoadConfirmed,
                    "exited" => manager_core::domain::SessionState::Exited,
                    "failed" => manager_core::domain::SessionState::Failed,
                    "verification_unavailable" => {
                        manager_core::domain::SessionState::VerificationUnavailable
                    }
                    _ => manager_core::domain::SessionState::Starting,
                };

                Ok(LegacySession {
                    id,
                    game_id,
                    setup_id,
                    launched_at,
                    pid,
                    state,
                    expected_mod_ids,
                    log_baseline_time: None,
                    log_baseline,
                    verification_result,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?
        } else {
            stmt.query_row([], |row| {
                let id: String = row.get(0)?;
                let game_id: String = row.get(1)?;
                let setup_id: String = row.get(2)?;
                let date_str: String = row.get(3)?;
                let pid: Option<u32> = row.get(4)?;
                let state_str: String = row.get(5)?;
                let expected_json: String = row.get(6)?;
                let verif_json: Option<String> = row.get(7)?;
                let baseline_json: Option<String> = row.get(8)?;

                let launched_at = parse_db_datetime(&date_str, 3)?;
                let expected_mod_ids: Vec<String> =
                    serde_json::from_str(&expected_json).unwrap_or_default();
                let verification_result: Option<manager_core::domain::VerificationResult> =
                    verif_json.and_then(|v| serde_json::from_str(&v).ok());
                let log_baseline: Option<manager_core::launch::SessionVerificationBaseline> =
                    baseline_json.and_then(|b| serde_json::from_str(&b).ok());

                let state = match state_str.as_str() {
                    "running_unverified" => manager_core::domain::SessionState::RunningUnverified,
                    "mod_load_confirmed" => manager_core::domain::SessionState::ModLoadConfirmed,
                    "exited" => manager_core::domain::SessionState::Exited,
                    "failed" => manager_core::domain::SessionState::Failed,
                    "verification_unavailable" => {
                        manager_core::domain::SessionState::VerificationUnavailable
                    }
                    _ => manager_core::domain::SessionState::Starting,
                };

                Ok(LegacySession {
                    id,
                    game_id,
                    setup_id,
                    launched_at,
                    pid,
                    state,
                    expected_mod_ids,
                    log_baseline_time: None,
                    log_baseline,
                    verification_result,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?
        };

        Ok(session)
    }

    fn update_launch_session(&self, session: &LegacySession) -> Result<(), String> {
        StateRepository::save_launch_session(self, session)
    }

    fn save_smapi_installation(&self, record: &SmapiInstallationRecord) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO smapi_installations (id, game_id, release_version, adapter_version, observed_version, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(game_id) DO UPDATE SET release_version=excluded.release_version, observed_version=excluded.observed_version, installed_at=excluded.installed_at",
            params![
                record.id,
                record.game_id,
                record.release_version,
                record.adapter_version,
                record.observed_version,
                record.installed_at.to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_smapi_installation(
        &self,
        game_id: &str,
    ) -> Result<Option<SmapiInstallationRecord>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT id, game_id, release_version, adapter_version, observed_version, installed_at FROM smapi_installations WHERE game_id = ?1")
            .map_err(|e| e.to_string())?;

        let record = stmt
            .query_row(params![game_id], |row| {
                let id: String = row.get(0)?;
                let game_id: String = row.get(1)?;
                let release_version: String = row.get(2)?;
                let adapter_version: String = row.get(3)?;
                let observed_version: Option<String> = row.get(4)?;
                let date_str: String = row.get(5)?;
                let installed_at = DateTime::parse_from_rfc3339(&date_str)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                Ok(SmapiInstallationRecord {
                    id,
                    game_id,
                    release_version,
                    adapter_version,
                    observed_version,
                    installed_at,
                })
            })
            .optional()
            .map_err(|e| e.to_string())?;

        Ok(record)
    }

    fn save_window_geometry(&self, geom: &WindowGeometry) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO window_geometry (id, schema_version, width, height, x, y, is_maximized)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET width=excluded.width, height=excluded.height, x=excluded.x, y=excluded.y, is_maximized=excluded.is_maximized",
            params![
                geom.schema_version,
                geom.width,
                geom.height,
                geom.x,
                geom.y,
                if geom.is_maximized { 1 } else { 0 },
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn get_window_geometry(&self) -> Result<Option<WindowGeometry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT schema_version, width, height, x, y, is_maximized FROM window_geometry WHERE id = 1")
            .map_err(|e| e.to_string())?;

        let geom = stmt
            .query_row([], |row| {
                let schema_version: u32 = row.get(0)?;
                let width: u32 = row.get(1)?;
                let height: u32 = row.get(2)?;
                let x: i32 = row.get(3)?;
                let y: i32 = row.get(4)?;
                let max_int: i32 = row.get(5)?;

                Ok(WindowGeometry {
                    schema_version,
                    width,
                    height,
                    x,
                    y,
                    is_maximized: max_int != 0,
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
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        for mod_item in installed_mods {
            let inv_json = serde_json::to_string(&mod_item.file_inventory).unwrap_or_default();
            tx.execute(
                "INSERT INTO installed_mods (id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    mod_item.id,
                    mod_item.setup_id,
                    mod_item.package_id,
                    mod_item.unique_id,
                    mod_item.name,
                    mod_item.author,
                    mod_item.version,
                    mod_item.description,
                    mod_item.raw_manifest,
                    mod_item.relative_target_path,
                    inv_json,
                    mod_item.installed_at.to_rfc3339(),
                ],
            )
            .map_err(|e| e.to_string())?;
        }

        tx.execute(
            "UPDATE operations SET state = 'succeeded', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
            params![operation_id],
        )
        .map_err(|e| e.to_string())?;

        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    fn commit_bundle_remove_transaction(
        &self,
        operation_id: &str,
        installed_mod_ids: &[String],
    ) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;

        for id in installed_mod_ids {
            tx.execute("DELETE FROM installed_mods WHERE id = ?1", params![id])
                .map_err(|e| e.to_string())?;
        }

        tx.execute(
            "UPDATE operations SET state = 'succeeded', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
            params![operation_id],
        )
        .map_err(|e| e.to_string())?;

        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }
}
