//! A prepared v2 plan is frozen.
//!
//! Execution and restart recovery both read the semantic plan, so nothing may
//! rewrite it underneath them. These tests pin that boundary at the repository,
//! which is where an accidental rewrite would otherwise be invisible until the
//! next crash.

use manager_app::ports::repositories::OperationRepository;
use manager_core::ids::{GameInstallationId, OperationId, ProfileId};
use manager_core::operation::{
    Operation, OperationKind, OperationState, OPERATION_PLAN_SCHEMA_V1, OPERATION_PLAN_SCHEMA_V2,
};
use manager_infra::db::SqliteStateRepository;
use std::sync::Arc;

fn prepared_v2_operation() -> Operation {
    Operation {
        id: OperationId::new(),
        kind: OperationKind::ModInstall,
        state: OperationState::Prepared,
        game_installation_id: Some(GameInstallationId::new()),
        profile_id: Some(ProfileId::new()),
        expected_profile_revision: Some(7),
        plan_schema_version: OPERATION_PLAN_SCHEMA_V2,
        plan_json: r#"{"mod_folder_name":"Author.Mod"}"#.to_string(),
        progress_current: Some(3),
        progress_total: Some(6),
        error_code: None,
        error_json: None,
        cancellation_requested: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        completed_at: None,
    }
}

#[test]
fn execution_changes_state_without_rewriting_the_prepared_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("state.sqlite3")).unwrap());
    let operation = prepared_v2_operation();
    repo.create_operation(&operation).unwrap();

    // The engine moves the operation forward and then writes its bookkeeping
    // back through the same upsert.
    repo.update_operation_state(&operation.id, OperationState::Running, None, None)
        .unwrap();
    let mut rewritten = operation.clone();
    rewritten.state = OperationState::Committing;
    rewritten.plan_json = r#"{"mod_folder_name":"Someone.Else"}"#.to_string();
    rewritten.plan_schema_version = OPERATION_PLAN_SCHEMA_V1;
    rewritten.expected_profile_revision = Some(99);
    rewritten.kind = OperationKind::ModRemove;
    rewritten.progress_current = Some(4);
    rewritten.progress_total = Some(6);
    repo.save_operation(&rewritten).unwrap();

    let stored = repo.get_operation(&operation.id).unwrap().unwrap();
    assert_eq!(
        stored.state,
        OperationState::Committing,
        "engine bookkeeping still applies"
    );
    assert_eq!(stored.progress_current, Some(4));
    assert_eq!(stored.plan_json, operation.plan_json, "plan_json is frozen");
    assert_eq!(
        stored.plan_schema_version, OPERATION_PLAN_SCHEMA_V2,
        "plan_schema_version is frozen"
    );
    assert_eq!(
        stored.expected_profile_revision,
        Some(7),
        "the revision the plan was validated against is frozen"
    );
    assert_eq!(stored.kind, OperationKind::ModInstall, "kind is frozen");
}

#[test]
fn creating_over_an_existing_operation_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("state.sqlite3")).unwrap());
    let operation = prepared_v2_operation();
    repo.create_operation(&operation).unwrap();

    let duplicate = Operation {
        plan_json: r#"{"mod_folder_name":"Overwritten"}"#.to_string(),
        ..operation.clone()
    };
    let error = repo
        .create_operation(&duplicate)
        .expect_err("creating over a journaled operation must fail");
    assert_eq!(error.code, "OPERATION_ALREADY_EXISTS");

    let stored = repo.get_operation(&operation.id).unwrap().unwrap();
    assert_eq!(stored.plan_json, operation.plan_json);
}
