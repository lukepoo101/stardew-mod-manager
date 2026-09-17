//! Recovery semantics for the modern operation engine.
//!
//! These assertions protect the rule that recovery is only ever cleared when
//! filesystem/database consistency is restored or proven, never merely to make
//! the application usable.

use chrono::Utc;
use manager_app::ports::repositories::{
    GameInstallationRepository, OperationRepository, ProfileRepository, SmapiRepository,
};
use manager_app::services::OperationsService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{GameInstallationId, OperationId};
use manager_core::operation::{Operation, OperationKind, OperationState};
use manager_infra::archive::StagedContentVerifier;
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::launcher::DetachedGameLauncher;
use manager_infra::lock::FileInstanceLock;
use manager_infra::paths::AppPaths;
use manager_infra::smapi_adapter::ProcessSmapiInstaller;
use std::sync::Arc;

struct Harness {
    service: OperationsService,
    repo: Arc<SqliteStateRepository>,
    game_id: GameInstallationId,
    game_root: std::path::PathBuf,
    paths: AppPaths,
    _tmp: tempfile::TempDir,
}

fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("state.sqlite3")).unwrap());

    let game_root = tmp.path().join("Game");
    std::fs::create_dir_all(&game_root).unwrap();
    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: game_root.clone(),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();

    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let deployment = Arc::new(FilesystemDeploymentAdapter::new(paths.clone()));
    let smapi = Arc::new(ProcessSmapiInstaller::new(paths.smapi_cache_dir()));

    let service = OperationsService::new(
        Arc::new(manager_app::services::ResourceCoordinator::new()),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        deployment.clone(),
        deployment,
        Arc::new(StagedContentVerifier),
        Arc::new(DetachedGameLauncher::isolated()),
        Arc::new(FileInstanceLock::new(paths.lock_file_path())),
        repo.clone(),
        smapi,
        repo.clone(),
    );

    Harness {
        service,
        repo,
        game_id: game.id,
        game_root,
        paths,
        _tmp: tmp,
    }
}

fn recovery_required_smapi_operation(game_id: GameInstallationId) -> Operation {
    Operation {
        id: OperationId::new(),
        kind: OperationKind::SmapiSetup,
        state: OperationState::RecoveryRequired,
        game_installation_id: Some(game_id),
        profile_id: None,
        expected_profile_revision: None,
        plan_schema_version: 1,
        plan_json: serde_json::json!({ "release_policy_id": "pinned" }).to_string(),
        progress_current: None,
        progress_total: None,
        error_code: Some("RECONCILIATION_REQUIRED".to_string()),
        error_json: None,
        cancellation_requested: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        completed_at: None,
    }
}

fn write_smapi_artifacts(game_root: &std::path::Path, version: Option<&str>) {
    std::fs::write(game_root.join("StardewModdingAPI.dll"), b"dll").unwrap();
    std::fs::create_dir_all(game_root.join("smapi-internal")).unwrap();
    if let Some(version) = version {
        std::fs::write(
            game_root.join("StardewModdingAPI.deps.json"),
            format!(
                "{{\"targets\":{{\"net6.0/linux-x64\":{{\"StardewModdingAPI/{version}\":{{}}}}}}}}"
            ),
        )
        .unwrap();
    }
}

#[test]
fn an_interrupted_smapi_setup_with_filesystem_evidence_is_recovered() {
    let h = harness();
    write_smapi_artifacts(&h.game_root, Some("4.1.10"));
    let operation = recovery_required_smapi_operation(h.game_id);
    let operation_id = operation.id;
    h.repo.save_operation(&operation).unwrap();

    h.service.retry_recovery().unwrap();

    let recovered = OperationRepository::get_operation(&*h.repo, &operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.state, OperationState::Succeeded);
    assert_eq!(
        recovered.error_code.as_deref(),
        Some("RECOVERED_SMAPI_STATE")
    );
    assert!(
        SmapiRepository::get_smapi_installation(&*h.repo, &h.game_id)
            .unwrap()
            .is_some()
    );
}

#[test]
fn an_interrupted_smapi_setup_without_artifacts_becomes_a_terminal_failure() {
    let h = harness();
    let operation = recovery_required_smapi_operation(h.game_id);
    let operation_id = operation.id;
    h.repo.save_operation(&operation).unwrap();

    h.service.retry_recovery().unwrap();

    let recovered = OperationRepository::get_operation(&*h.repo, &operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.state, OperationState::Failed);
    assert_eq!(recovered.error_code.as_deref(), Some("SMAPI_NOT_PRESENT"));
}

#[test]
fn an_interrupted_smapi_setup_with_an_unreadable_version_stays_a_terminal_failure() {
    let h = harness();
    write_smapi_artifacts(&h.game_root, None);
    let operation = recovery_required_smapi_operation(h.game_id);
    let operation_id = operation.id;
    h.repo.save_operation(&operation).unwrap();

    h.service.retry_recovery().unwrap();

    let recovered = OperationRepository::get_operation(&*h.repo, &operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.state, OperationState::Failed);
    assert_eq!(
        recovered.error_code.as_deref(),
        Some("SMAPI_RECOVERY_VERSION_UNKNOWN")
    );
}

#[test]
fn an_interrupted_install_without_a_published_folder_is_conservatively_reconciled() {
    use manager_core::profile::Profile;

    let h = harness();
    let profile = Profile::new(h.game_id, "Default");
    h.repo.save_profile(&profile).unwrap();

    let operation = Operation {
        kind: OperationKind::ModInstall,
        state: OperationState::RecoveryRequired,
        profile_id: Some(profile.id),
        game_installation_id: Some(h.game_id),
        plan_json: serde_json::json!({ "mod_folder_name": "Author.Mod" }).to_string(),
        error_code: Some("RECONCILIATION_REQUIRED".to_string()),
        ..recovery_required_smapi_operation(h.game_id)
    };
    let operation_id = operation.id;
    h.repo.save_operation(&operation).unwrap();

    h.service.retry_recovery().unwrap();

    // Nothing was published, so the journal is cleared to a terminal failure that
    // explicitly tells the user to retry with a fresh plan.
    let reconciled = OperationRepository::get_operation(&*h.repo, &operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(reconciled.state, OperationState::Failed);
    assert_eq!(
        reconciled.error_code.as_deref(),
        Some("RECOVERED_TO_TERMINAL_STATE")
    );
    assert!(!h
        .paths
        .profile_mods_dir(&profile.id)
        .join("Author.Mod")
        .exists());
}

#[test]
fn the_operation_repository_excludes_terminal_states_from_unresolved_work() {
    let h = harness();

    let running = Operation {
        state: OperationState::Running,
        ..recovery_required_smapi_operation(h.game_id)
    };
    let failed = Operation {
        state: OperationState::Failed,
        ..recovery_required_smapi_operation(h.game_id)
    };
    let rolled_back = Operation {
        state: OperationState::RolledBack,
        ..recovery_required_smapi_operation(h.game_id)
    };

    h.repo.save_operation(&running).unwrap();
    h.repo.save_operation(&failed).unwrap();
    h.repo.save_operation(&rolled_back).unwrap();

    let unresolved = OperationRepository::list_unresolved_operations(&*h.repo).unwrap();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].id, running.id);
}

#[test]
fn an_unresolved_profile_operation_blocks_mod_mutations_until_it_is_reconciled() {
    use manager_core::profile::Profile;

    let h = harness();
    let profile = Profile::new(h.game_id, "Default");
    h.repo.save_profile(&profile).unwrap();

    let operation = Operation {
        kind: OperationKind::ModRemove,
        state: OperationState::RecoveryRequired,
        profile_id: Some(profile.id),
        game_installation_id: Some(h.game_id),
        plan_json: serde_json::json!({ "deployment_rel_path": "Author.Mod" }).to_string(),
        error_code: Some("RECONCILIATION_REQUIRED".to_string()),
        ..recovery_required_smapi_operation(h.game_id)
    };
    h.repo.save_operation(&operation).unwrap();

    // The deployment directory does not exist anywhere, so consistency cannot be
    // proven and recovery must be retained rather than cleared.
    let error = h.service.retry_recovery().unwrap_err();
    assert!(
        !error.to_string().is_empty(),
        "recovery failure must be reported"
    );
    let retained = OperationRepository::get_operation(&*h.repo, &operation.id)
        .unwrap()
        .unwrap();
    assert_eq!(retained.state, OperationState::RecoveryRequired);

    // The staging directory is never a deployment target, so nothing was written.
    assert!(!h
        .paths
        .profile_mods_dir(&profile.id)
        .join("Author.Mod")
        .exists());
}
