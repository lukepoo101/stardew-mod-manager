//! A frozen profile refuses installs and removals through the production
//! commands, including a preview prepared before it was frozen.
#![cfg(target_os = "linux")]

use manager_app::ports::repositories::{
    GameInstallationRepository, OperationRepository, ProfileRepository,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{GameInstallationId, OperationId};
use manager_core::operation::{Operation, OperationKind, OperationState};
use manager_core::profile::Profile;
use manager_infra::paths::AppPaths;
use serde_json::{json, Value};
use stardew_mod_manager::state::AppState;
use tauri::Manager;

fn try_invoke(
    window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    tauri::test::get_ipc_response(
        window,
        tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        },
    )
    .map(|value| value.deserialize().unwrap())
}

#[test]
fn a_frozen_profile_refuses_installs_until_unfrozen() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let state = AppState::new_with_expected_smapi_hash(paths, Some("test")).unwrap();
    let app = stardew_mod_manager::configure(tauri::test::mock_builder(), state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let state = app.state::<AppState>();
    let state = state.inner();

    let now = "2026-09-13T00:00:00Z".parse().unwrap();
    let game_id = GameInstallationId::new();
    state
        .repo
        .save_game(&GameInstallation {
            id: game_id,
            canonical_root: tmp.path().join("game"),
            operating_system: OperatingSystem::Linux,
            storefront: Storefront::Steam,
            management_mode: ManagementMode::Managed,
            created_at: now,
        })
        .unwrap();
    let profile = Profile::new(game_id, "Co-op");
    state.repo.save_profile(&profile).unwrap();
    let pid = profile.id.to_string();

    // A preview prepared before the freeze.
    let operation_id = OperationId::new();
    state
        .repo
        .save_operation(&Operation {
            id: operation_id,
            kind: OperationKind::ModInstall,
            state: OperationState::Draft,
            game_installation_id: Some(game_id),
            profile_id: Some(profile.id),
            expected_profile_revision: Some(profile.revision),
            plan_schema_version: 1,
            plan_json: "{}".into(),
            progress_current: None,
            progress_total: None,
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
        .unwrap();

    let frozen = try_invoke(
        &window,
        "freeze_profile",
        json!({"profileId": pid, "reason": "Saturday co-op"}),
    )
    .expect("freeze");
    assert_eq!(frozen["reason"], "Saturday co-op");
    assert!(
        try_invoke(&window, "get_profile_freeze", json!({"profileId": pid}))
            .unwrap()
            .is_object()
    );

    let refused = try_invoke(
        &window,
        "execute_operation",
        json!({"operationId": operation_id.to_string()}),
    )
    .expect_err("a frozen profile must not change");
    assert_eq!(refused["code"], "PROFILE_FROZEN");
    assert!(refused["summary"]
        .as_str()
        .unwrap()
        .contains("Saturday co-op"));
    assert_eq!(
        state
            .repo
            .get_operation(&operation_id)
            .unwrap()
            .unwrap()
            .state,
        OperationState::Draft
    );

    let archive = tmp.path().join("mod.zip");
    std::fs::write(&archive, b"not inspected").unwrap();
    let refused = try_invoke(
        &window,
        "inspect_package_for_install",
        json!({"archivePath": archive.to_string_lossy(), "profileId": pid}),
    )
    .expect_err("no new previews either");
    assert_eq!(refused["code"], "PROFILE_FROZEN");

    try_invoke(&window, "unfreeze_profile", json!({"profileId": pid})).expect("unfreeze");
    assert!(
        try_invoke(&window, "get_profile_freeze", json!({"profileId": pid}))
            .unwrap()
            .is_null()
    );
    // Unfreezing does nothing else: the old preview is still just a draft.
    assert_eq!(
        state
            .repo
            .get_operation(&operation_id)
            .unwrap()
            .unwrap()
            .state,
        OperationState::Draft
    );
    let _ = state.repo.get_profile(&profile.id).unwrap().unwrap();
}
