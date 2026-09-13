use manager_core::{domain::*, install::*, ports::*, use_cases::CoreUseCases};
use manager_infra::{
    archive::{PendingInspectionStore, SafeZipExtractor},
    db::SqliteStateRepository,
    launcher::DetachedGameLauncher,
    lock::FileInstanceLock,
    log_reader::SmapiSessionLogReader,
    package_store::FilesystemPackageStore,
    smapi_adapter::ProcessSmapiInstaller,
};
use std::{io::Write, path::Path};
use zip::{write::SimpleFileOptions, ZipWriter};
type Cases = CoreUseCases<
    SqliteStateRepository,
    FilesystemPackageStore,
    ProcessSmapiInstaller,
    DetachedGameLauncher,
    SmapiSessionLogReader,
    FileInstanceLock,
>;
fn fixture(root: &Path) -> (Cases, InstallPlan) {
    let repo = SqliteStateRepository::new(root.join("state.db")).unwrap();
    repo.save_game(&GameInstallation {
        id: "game".into(),
        canonical_root: root.join("game"),
        platform_kind: StoreKind::ManualFolder,
        detected_version: None,
        validated_at: chrono::Utc::now(),
        is_fresh: true,
        is_managed: false,
        validation_error: None,
    })
    .unwrap();
    repo.save_setup(&Setup {
        id: "setup".into(),
        game_id: "game".into(),
        display_name: "test".into(),
        relative_mods_dir: "Mods".into(),
        created_at: chrono::Utc::now(),
    })
    .unwrap();
    let path = root.join("mod.zip");
    let mut z = ZipWriter::new(std::fs::File::create(&path).unwrap());
    z.start_file("manifest.json", SimpleFileOptions::default())
        .unwrap();
    z.write_all(br#"{"Name":"Test","Author":"A","Version":"1.0.0","UniqueID":"A.Test","EntryDll":"test.dll"}"#).unwrap();
    z.start_file("test.dll", SimpleFileOptions::default())
        .unwrap();
    z.write_all(b"original").unwrap();
    z.finish().unwrap();
    let plan = SafeZipExtractor::inspect_and_stage(&path, "setup", &root.join("staging"), &repo)
        .unwrap()
        .plan;
    (
        CoreUseCases::new(
            repo,
            FilesystemPackageStore::new(root.join("packages")),
            ProcessSmapiInstaller::new(root.join("cache")),
            DetachedGameLauncher::isolated(),
            SmapiSessionLogReader::new(None),
            FileInstanceLock::new(root.join("lock")),
        ),
        plan,
    )
}
#[test]
fn database_failures_remain_recoverable_for_install_and_remove() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let (cases, plan) = fixture(r);
    let connection = rusqlite::Connection::open(r.join("state.db")).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_install BEFORE INSERT ON installed_mods BEGIN SELECT RAISE(FAIL, 'injected failure'); END;").unwrap();
    assert!(cases
        .commit_mod_install(&plan, &r.join("staging"), &r.join("Mods"))
        .is_err());
    assert_eq!(cases.repo.list_unresolved_operations().unwrap().len(), 1);
    assert!(cases
        .recover_operations(&r.join("Mods"), &r.join("staging"), &r.join("recovery"))
        .is_err());
    assert_eq!(cases.repo.list_unresolved_operations().unwrap().len(), 1);
    connection
        .execute_batch("DROP TRIGGER fail_install;")
        .unwrap();
    assert_eq!(
        cases
            .recover_operations(&r.join("Mods"), &r.join("staging"), &r.join("recovery"))
            .unwrap(),
        1
    );
    assert_eq!(
        cases
            .recover_operations(&r.join("Mods"), &r.join("staging"), &r.join("recovery"))
            .unwrap(),
        0
    );
    let item = cases.repo.list_installed_mods("setup").unwrap().remove(0);
    connection.execute_batch("CREATE TRIGGER fail_remove BEFORE DELETE ON installed_mods BEGIN SELECT RAISE(FAIL, 'injected failure'); END;").unwrap();
    assert!(cases
        .remove_mod(&item.id, "setup", &r.join("Mods"), &r.join("recovery"))
        .is_err());
    assert_eq!(cases.repo.list_unresolved_operations().unwrap().len(), 1);
    connection
        .execute_batch("DROP TRIGGER fail_remove;")
        .unwrap();
    cases
        .recover_operations(&r.join("Mods"), &r.join("staging"), &r.join("recovery"))
        .unwrap();
    assert!(cases.repo.list_installed_mods("setup").unwrap().is_empty());
}
#[test]
fn tampered_staging_and_expired_inspection() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let (cases, plan) = fixture(r);
    let staged = r.join("staging").join(&plan.plan_id);
    std::fs::write(
        staged.join(&plan.mod_folder_name).join("test.dll"),
        b"tampered",
    )
    .unwrap();
    assert!(cases
        .commit_mod_install(&plan, &r.join("staging"), &r.join("Mods"))
        .unwrap_err()
        .contains("Hash mismatch"));
    assert!(!r.join("Mods").join(&plan.mod_folder_name).exists());
    let store = PendingInspectionStore::with_ttl(std::time::Duration::ZERO);
    store.insert_staged(plan.clone(), Some(staged.clone()));
    assert!(store.take(&plan.plan_id).is_none());
    assert!(!staged.exists());
}
#[test]
fn refuses_to_signal_untracked_process() {
    let mut child = std::process::Command::new("sleep")
        .arg("10")
        .spawn()
        .unwrap();
    let result = DetachedGameLauncher::isolated().terminate_game(Some(child.id()));
    let alive = child.try_wait().unwrap().is_none();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(result.is_err());
    assert!(alive);
}
#[test]
fn log_errors_are_not_load_evidence() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("log");
    std::fs::write(&p,"[12:00:00 INFO SMAPI] Log started at invalid UTC\n[12:00:01 ERROR SMAPI] A.Broken could not be loaded").unwrap();
    let reader = SmapiSessionLogReader::new(Some(p));
    let baseline = reader.capture_baseline().unwrap();
    assert!(!reader
        .verify_session(&baseline, &["A.Broken".into()], &[])
        .is_ok_and(|r| r.all_mods_confirmed));
}
#[test]
fn bundle_versions_are_checked() {
    let a=manager_core::manifest::parse_manifest(r#"{"Name":"A","Version":"1.0","UniqueID":"A.A","EntryDll":"a.dll","Dependencies":[{"UniqueID":"B.B","MinimumVersion":"2.0"}]}"#).unwrap();
    let b = manager_core::manifest::parse_manifest(
        r#"{"Name":"B","Version":"1.0","UniqueID":"B.B","EntryDll":"b.dll"}"#,
    )
    .unwrap();
    assert!(
        !manager_core::dependency::evaluate_bundle_dependencies(&[a, b], &[], Some("4.1.10"))
            .is_installable
    );
}

#[test]
fn selecting_existing_game_preserves_setup() {
    let t = tempfile::tempdir().unwrap();
    let (cases, _) = fixture(t.path());
    let game = cases.repo.get_game("game").unwrap().unwrap();
    cases.repo.save_game(&game).unwrap();
    assert!(cases.repo.get_setup("setup").unwrap().is_some());
}

#[test]
fn smapi_setup_reconciliation_and_terminal_operations_not_unresolved() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let (cases, _) = fixture(r);

    let game_dir = r.join("game");
    std::fs::create_dir_all(&game_dir).unwrap();

    // 1. Create a failed smapi_setup operation and verify it is not unresolved
    let failed_op = Operation {
        id: "op-failed".into(),
        kind: OperationKind::SmapiSetup,
        state: OperationState::Failed,
        plan_json: serde_json::json!({ "game_id": "game", "version": "4.1.10" }).to_string(),
        error_json: Some("Prior failure".into()),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        schema_version: 1,
    };
    cases.repo.save_operation(&failed_op).unwrap();
    assert_eq!(cases.repo.list_unresolved_operations().unwrap().len(), 0);

    // 2. Create a running smapi_setup operation when SMAPI binary is missing
    let running_op_1 = Operation {
        id: "op-running-1".into(),
        kind: OperationKind::SmapiSetup,
        state: OperationState::Running,
        plan_json: serde_json::json!({ "game_id": "game", "version": "4.1.10" }).to_string(),
        error_json: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        schema_version: 1,
    };
    cases.repo.save_operation(&running_op_1).unwrap();
    assert_eq!(cases.repo.list_unresolved_operations().unwrap().len(), 1);

    // Recovering should reconcile it to Failed because binary doesn't exist
    assert_eq!(
        cases
            .recover_operations(&r.join("Mods"), &r.join("staging"), &r.join("recovery"))
            .unwrap(),
        1
    );
    assert_eq!(cases.repo.list_unresolved_operations().unwrap().len(), 0);
    let op1 = cases.repo.get_operation("op-running-1").unwrap().unwrap();
    assert_eq!(op1.state, OperationState::Failed);

    // 3. Create a running smapi_setup operation when SMAPI binary exists
    std::fs::write(game_dir.join("StardewModdingAPI"), b"fake binary").unwrap();
    let running_op_2 = Operation {
        id: "op-running-2".into(),
        kind: OperationKind::SmapiSetup,
        state: OperationState::Running,
        plan_json: serde_json::json!({ "game_id": "game", "version": "4.1.10" }).to_string(),
        error_json: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        schema_version: 1,
    };
    cases.repo.save_operation(&running_op_2).unwrap();
    assert_eq!(cases.repo.list_unresolved_operations().unwrap().len(), 1);

    // Recovering should reconcile it to Completed because binary exists
    assert_eq!(
        cases
            .recover_operations(&r.join("Mods"), &r.join("staging"), &r.join("recovery"))
            .unwrap(),
        1
    );
    assert_eq!(cases.repo.list_unresolved_operations().unwrap().len(), 0);
    let op2 = cases.repo.get_operation("op-running-2").unwrap().unwrap();
    assert_eq!(op2.state, OperationState::Completed);
}

#[test]
fn test_modern_operation_repository_excludes_terminal_states() {
    use manager_app::ports::repositories::OperationRepository;
    use manager_core::ids::OperationId;
    use manager_core::operation::{
        Operation as ModernOp, OperationKind as ModernOpKind, OperationState as ModernOpState,
    };

    let t = tempfile::tempdir().unwrap();
    let repo = SqliteStateRepository::new(t.path().join("state.sqlite3")).unwrap();

    let op_running = ModernOp {
        id: OperationId::new(),
        kind: ModernOpKind::ModInstall,
        state: ModernOpState::Running,
        plan_json: "{}".into(),
        error_json: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        plan_schema_version: 1,
        game_installation_id: None,
        profile_id: None,
        expected_profile_revision: None,
        progress_current: None,
        progress_total: None,
        error_code: None,
        cancellation_requested: false,
        completed_at: None,
    };
    let op_failed = ModernOp {
        id: OperationId::new(),
        kind: ModernOpKind::ModInstall,
        state: ModernOpState::Failed,
        plan_json: "{}".into(),
        error_json: Some("err".into()),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        plan_schema_version: 1,
        game_installation_id: None,
        profile_id: None,
        expected_profile_revision: None,
        progress_current: None,
        progress_total: None,
        error_code: None,
        cancellation_requested: false,
        completed_at: None,
    };
    let op_rolled_back = ModernOp {
        id: OperationId::new(),
        kind: ModernOpKind::ModInstall,
        state: ModernOpState::RolledBack,
        plan_json: "{}".into(),
        error_json: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        plan_schema_version: 1,
        game_installation_id: None,
        profile_id: None,
        expected_profile_revision: None,
        progress_current: None,
        progress_total: None,
        error_code: None,
        cancellation_requested: false,
        completed_at: None,
    };

    OperationRepository::save_operation(&repo, &op_running).unwrap();
    OperationRepository::save_operation(&repo, &op_failed).unwrap();
    OperationRepository::save_operation(&repo, &op_rolled_back).unwrap();

    let unresolved = OperationRepository::list_unresolved_operations(&repo).unwrap();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].id, op_running.id);
}
