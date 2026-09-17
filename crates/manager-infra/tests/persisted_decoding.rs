//! Persisted semantic values must either decode or fail loudly.
//!
//! These tests write deliberately invalid literals into a real SQLite database
//! and prove the reader refuses to reinterpret them: an unknown operation state
//! never becomes Failed, an unknown operation kind never becomes SmapiSetup, the
//! offending row is never rewritten, and recovery never acts on a fabricated
//! interpretation of stored evidence.

use manager_app::error::AppErrorCategory;
use manager_app::ports::repositories::{
    GameInstallationRepository, OperationRepository, ProfileRepository,
};
use manager_app::services::OperationsService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::GameInstallationId;
use manager_core::operation::{
    AccessMode, Operation, OperationKind, OperationResource, OperationState, OperationStep,
    OperationStepState, ResourceKind,
};
use manager_core::profile::Profile;
use manager_infra::archive::StagedContentVerifier;
use manager_infra::db::decoding;
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::launcher::DetachedGameLauncher;
use manager_infra::lock::FileInstanceLock;
use manager_infra::paths::AppPaths;
use manager_infra::smapi_adapter::ProcessSmapiInstaller;
use rusqlite::{params, Connection};
use std::sync::Arc;

struct Harness {
    repo: Arc<SqliteStateRepository>,
    db_path: std::path::PathBuf,
    game_id: GameInstallationId,
    paths: AppPaths,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn connect(&self) -> Connection {
        Connection::open(&self.db_path).expect("raw connection")
    }

    fn raw_scalar(&self, query: &str, id: &str) -> String {
        self.connect()
            .query_row(query, params![id], |row| row.get::<_, String>(0))
            .expect("raw read")
    }

    fn execute(&self, sql: &str, value: &str, id: &str) {
        self.connect()
            .execute(sql, params![value, id])
            .expect("raw write");
    }

    fn service(&self) -> OperationsService {
        let deployment = Arc::new(FilesystemDeploymentAdapter::new(self.paths.clone()));
        OperationsService::new(
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            deployment.clone(),
            deployment,
            Arc::new(StagedContentVerifier),
            Arc::new(DetachedGameLauncher::isolated()),
            Arc::new(FileInstanceLock::new(self.paths.lock_file_path())),
            self.repo.clone(),
            Arc::new(ProcessSmapiInstaller::new(self.paths.smapi_cache_dir())),
            self.repo.clone(),
        )
    }
}

fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("state.sqlite3");
    let repo = Arc::new(SqliteStateRepository::new(&db_path).unwrap());

    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: chrono::Utc::now(),
    };
    repo.save_game(&game).unwrap();

    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    Harness {
        repo,
        db_path,
        game_id: game.id,
        paths,
        _tmp: tmp,
    }
}

fn running_install(
    game_id: GameInstallationId,
    profile_id: manager_core::ids::ProfileId,
) -> Operation {
    Operation {
        id: manager_core::ids::OperationId::new(),
        kind: OperationKind::ModInstall,
        state: OperationState::Running,
        game_installation_id: Some(game_id),
        profile_id: Some(profile_id),
        expected_profile_revision: Some(1),
        plan_schema_version: 1,
        plan_json: serde_json::json!({ "mod_folder_name": "Author.Mod" }).to_string(),
        progress_current: None,
        progress_total: None,
        error_code: None,
        error_json: None,
        cancellation_requested: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        completed_at: None,
    }
}

#[test]
fn an_unknown_operation_state_fails_the_read_instead_of_becoming_failed() {
    let h = harness();
    let profile = Profile::new(h.game_id, "Default");
    h.repo.save_profile(&profile).unwrap();
    let operation = running_install(h.game_id, profile.id);
    let operation_id = operation.id.to_string();
    h.repo.save_operation(&operation).unwrap();

    h.execute(
        "UPDATE operations SET state = ?1 WHERE id = ?2",
        "definitely-not-a-state",
        &operation_id,
    );

    let error = OperationRepository::get_operation(&*h.repo, &operation.id)
        .expect_err("an unknown persisted state must not decode");
    assert_eq!(error.code, decoding::PERSISTED_OPERATION_STATE_INVALID);
    assert_eq!(error.category, AppErrorCategory::Storage);
    assert_eq!(error.summary, "Stored operation data could not be read");
    let details = error.technical_details.expect("diagnostics");
    assert!(details.contains("operations"), "{details}");
    assert!(details.contains("definitely-not-a-state"), "{details}");

    // The reader must not repair evidence it did not understand.
    assert_eq!(
        h.raw_scalar("SELECT state FROM operations WHERE id = ?1", &operation_id),
        "definitely-not-a-state"
    );
}

#[test]
fn an_unknown_operation_kind_never_becomes_smapi_setup() {
    let h = harness();
    let profile = Profile::new(h.game_id, "Default");
    h.repo.save_profile(&profile).unwrap();
    let operation = running_install(h.game_id, profile.id);
    let operation_id = operation.id.to_string();
    h.repo.save_operation(&operation).unwrap();

    h.execute(
        "UPDATE operations SET kind = ?1 WHERE id = ?2",
        "mystery_kind",
        &operation_id,
    );

    let error = OperationRepository::get_operation(&*h.repo, &operation.id)
        .expect_err("an unknown persisted kind must not decode");
    assert_eq!(error.code, decoding::PERSISTED_OPERATION_KIND_INVALID);
    assert_eq!(error.category, AppErrorCategory::Storage);
    assert_eq!(
        h.raw_scalar("SELECT kind FROM operations WHERE id = ?1", &operation_id),
        "mystery_kind"
    );
}

#[test]
fn an_unknown_step_state_fails_the_read() {
    let h = harness();
    let profile = Profile::new(h.game_id, "Default");
    h.repo.save_profile(&profile).unwrap();
    let operation = running_install(h.game_id, profile.id);
    let operation_id = operation.id.to_string();
    h.repo.save_operation(&operation).unwrap();
    h.repo
        .save_operation_step(&OperationStep {
            operation_id: operation.id,
            step_index: 1,
            step_kind: "publish_deployment".to_string(),
            state: OperationStepState::Running,
            payload_json: "{}".to_string(),
            started_at: None,
            completed_at: None,
            error_json: None,
        })
        .unwrap();

    h.connect()
        .execute(
            "UPDATE operation_steps SET state = ?1 WHERE operation_id = ?2",
            params!["halfway", &operation_id],
        )
        .unwrap();

    let error = OperationRepository::list_operation_steps(&*h.repo, &operation.id)
        .expect_err("an unknown persisted step state must not decode");
    assert_eq!(error.code, decoding::PERSISTED_STEP_STATE_INVALID);
    assert_eq!(
        h.raw_scalar(
            "SELECT state FROM operation_steps WHERE operation_id = ?1",
            &operation_id
        ),
        "halfway"
    );
}

#[test]
fn an_unknown_resource_kind_or_access_mode_fails_the_read() {
    let h = harness();
    let profile = Profile::new(h.game_id, "Default");
    h.repo.save_profile(&profile).unwrap();
    let operation = running_install(h.game_id, profile.id);
    let operation_id = operation.id.to_string();
    h.repo.save_operation(&operation).unwrap();
    h.repo
        .save_operation_resource(&OperationResource {
            operation_id: operation.id,
            resource_kind: ResourceKind::Profile,
            resource_id: profile.id.to_string(),
            access_mode: AccessMode::Write,
        })
        .unwrap();

    h.execute(
        "UPDATE operation_resources SET resource_kind = ?1 WHERE operation_id = ?2",
        "weapon",
        &operation_id,
    );
    let error = OperationRepository::list_operation_resources(&*h.repo, &operation.id)
        .expect_err("an unknown persisted resource kind must not decode");
    assert_eq!(error.code, decoding::PERSISTED_RESOURCE_KIND_INVALID);

    h.execute(
        "UPDATE operation_resources SET resource_kind = 'profile', access_mode = ?1 WHERE operation_id = ?2",
        "sometimes",
        &operation_id,
    );
    let error = OperationRepository::list_operation_resources(&*h.repo, &operation.id)
        .expect_err("an unknown persisted access mode must not decode");
    assert_eq!(error.code, decoding::PERSISTED_ACCESS_MODE_INVALID);
}

#[test]
fn an_unknown_game_installation_semantic_value_fails_the_read() {
    let h = harness();

    h.execute(
        "UPDATE game_installations SET storefront = ?1 WHERE id = ?2",
        "epic",
        &h.game_id.to_string(),
    );
    let error = GameInstallationRepository::get_game(&*h.repo, &h.game_id)
        .expect_err("an unknown storefront must not decode");
    assert_eq!(error.code, decoding::PERSISTED_STOREFRONT_INVALID);
    assert_eq!(
        h.raw_scalar(
            "SELECT storefront FROM game_installations WHERE id = ?1",
            &h.game_id.to_string()
        ),
        "epic"
    );

    // A deliberate "unknown" literal is a real domain value, not a failure.
    h.execute(
        "UPDATE game_installations SET storefront = ?1 WHERE id = ?2",
        "unknown",
        &h.game_id.to_string(),
    );
    let game = GameInstallationRepository::get_game(&*h.repo, &h.game_id)
        .unwrap()
        .expect("game");
    assert_eq!(game.storefront, Storefront::Unknown);

    h.execute(
        "UPDATE game_installations SET operating_system = ?1 WHERE id = ?2",
        "beos",
        &h.game_id.to_string(),
    );
    let error = GameInstallationRepository::get_game(&*h.repo, &h.game_id)
        .expect_err("an unknown operating system must not decode");
    assert_eq!(error.code, decoding::PERSISTED_OPERATING_SYSTEM_INVALID);
}

#[test]
fn an_unknown_profile_state_fails_the_read() {
    let h = harness();
    let profile = Profile::new(h.game_id, "Default");
    let profile_id = profile.id.to_string();
    h.repo.save_profile(&profile).unwrap();

    h.execute(
        "UPDATE profiles SET state = ?1 WHERE id = ?2",
        "suspended",
        &profile_id,
    );

    let error = ProfileRepository::get_profile(&*h.repo, &profile.id)
        .expect_err("an unknown profile state must not decode");
    assert_eq!(error.code, decoding::PERSISTED_PROFILE_STATE_INVALID);
    assert_eq!(
        h.raw_scalar("SELECT state FROM profiles WHERE id = ?1", &profile_id),
        "suspended"
    );
}

#[test]
fn recovery_never_runs_against_a_fabricated_interpretation() {
    let h = harness();
    let profile = Profile::new(h.game_id, "Default");
    h.repo.save_profile(&profile).unwrap();
    let operation = running_install(h.game_id, profile.id);
    let operation_id = operation.id.to_string();
    h.repo.save_operation(&operation).unwrap();

    h.execute(
        "UPDATE operations SET state = ?1 WHERE id = ?2",
        "sort_of_running",
        &operation_id,
    );

    let error = h
        .service()
        .retry_recovery()
        .expect_err("recovery must refuse to act on an unreadable operation row");
    assert_eq!(error.code, decoding::PERSISTED_OPERATION_STATE_INVALID);
    assert_eq!(error.category, AppErrorCategory::Storage);

    // The row and the managed filesystem are untouched: nothing was reinterpreted
    // as Failed, and no compensating filesystem action was attempted.
    assert_eq!(
        h.raw_scalar("SELECT state FROM operations WHERE id = ?1", &operation_id),
        "sort_of_running"
    );
    assert!(!h
        .paths
        .profile_mods_dir(&profile.id)
        .join("Author.Mod")
        .exists());
}

#[test]
fn known_historical_persisted_values_still_decode() {
    let h = harness();
    let profile = Profile::new(h.game_id, "Default");
    h.repo.save_profile(&profile).unwrap();

    let operation = running_install(h.game_id, profile.id);
    let operation_id = operation.id.to_string();
    h.repo.save_operation(&operation).unwrap();

    for (raw, expected) in [
        ("completed", OperationState::Succeeded),
        ("recovering", OperationState::RecoveryRequired),
        ("pending", OperationState::Draft),
        ("recovery_required", OperationState::RecoveryRequired),
    ] {
        h.execute(
            "UPDATE operations SET state = ?1 WHERE id = ?2",
            raw,
            &operation_id,
        );
        let decoded = OperationRepository::get_operation(&*h.repo, &operation.id)
            .unwrap()
            .expect("operation");
        assert_eq!(decoded.state, expected, "raw state {raw}");
    }
}
