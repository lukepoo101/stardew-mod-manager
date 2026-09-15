use manager_infra::paths::AppPaths;
use serde_json::{json, Value};
use stardew_mod_manager::{configure, state::AppState};
use std::io::Write;

fn invoke(
    window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    body: Value,
) -> Value {
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
    .unwrap_or_else(|err| panic!("{cmd} failed: {err}"))
    .deserialize()
    .unwrap()
}

fn try_invoke(
    window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    body: Value,
) -> Result<Value, serde_json::Value> {
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
fn modern_onboarding_and_profile_commands_dispatch_through_production_handler() {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("game");
    std::fs::create_dir(&game).unwrap();
    std::fs::write(game.join("StardewValley"), b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(game.join("Stardew Valley.dll"), b"fixture").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        game.join("StardewValley"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let state = AppState::new_with_paths(AppPaths::new(
        tmp.path().join("data"),
        tmp.path().join("cache"),
    ))
    .unwrap();
    let app = configure(tauri::test::mock_builder(), state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    assert!(invoke(&window, "bootstrap", json!({}))["active_game_installation_id"].is_null());
    let inspection = invoke(
        &window,
        "validate_game_installation_path",
        json!({"path": game}),
    );
    assert_eq!(inspection["is_usable"], true);
    let registered = invoke(
        &window,
        "register_game_installation",
        json!({"path": game, "storefront": "steam"}),
    );
    let boot = invoke(&window, "bootstrap", json!({}));
    assert_eq!(boot["active_game_installation_id"], registered["id"]);
    assert!(boot["active_profile_id"].is_string());
    let profiles = invoke(&window, "list_profiles", json!({}));
    assert_eq!(profiles.as_array().unwrap().len(), 1);
    let created = invoke(
        &window,
        "create_profile",
        json!({"gameId": registered["id"], "name": "Seasonal"}),
    );
    invoke(
        &window,
        "activate_profile",
        json!({"profileId": created["id"]}),
    );
    let overview = invoke(&window, "get_active_profile_overview", json!({}));
    assert_eq!(overview["profile"]["name"], "Seasonal");
    let spare = invoke(
        &window,
        "create_profile",
        json!({"gameId": registered["id"], "name": "Seasonal spare"}),
    );
    invoke(
        &window,
        "archive_profile",
        json!({"profileId": spare["id"]}),
    );
    assert!(try_invoke(
        &window,
        "archive_profile",
        json!({"profileId": created["id"]})
    )
    .is_err());
    assert!(!invoke(&window, "list_profiles", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == spare["id"]));
    let archived = invoke(&window, "list_archived_profiles", json!({}));
    assert_eq!(archived.as_array().unwrap().len(), 1);
    assert_eq!(archived[0]["id"], spare["id"]);
    assert!(try_invoke(
        &window,
        "activate_profile",
        json!({"profileId": spare["id"]})
    )
    .is_err());
    invoke(
        &window,
        "restore_profile",
        json!({"profileId": spare["id"]}),
    );
    assert!(invoke(&window, "list_archived_profiles", json!({}))
        .as_array()
        .unwrap()
        .is_empty());
    invoke(
        &window,
        "activate_profile",
        json!({"profileId": spare["id"]}),
    );
    invoke(
        &window,
        "activate_profile",
        json!({"profileId": created["id"]}),
    );
    let smapi = invoke(&window, "get_smapi_status", json!({}));
    assert_eq!(smapi["is_installed"], false);
    let again = invoke(
        &window,
        "register_game_installation",
        json!({"path": game, "storefront": "steam"}),
    );
    assert_eq!(again["id"], registered["id"]);
    assert_eq!(
        invoke(&window, "list_profiles", json!({}))
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let archive = tmp.path().join("Example.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    zip.start_file(
        "Example/manifest.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(br#"{"Name":"Example", "Author":"Tests", "Version":"1.0.0", "UniqueID":"Tests.Example", "EntryDll":"Example.dll"}"#).unwrap();
    zip.start_file(
        "Example/Example.dll",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"fixture").unwrap();
    zip.finish().unwrap();
    let preview = invoke(
        &window,
        "inspect_package_for_install",
        json!({"archivePath": archive}),
    );
    assert_eq!(preview["dependencies_satisfied"], true);
    assert_eq!(
        preview["detected_components"][0]["unique_id"],
        "Tests.Example"
    );
    let operation = invoke(
        &window,
        "execute_operation",
        json!({"operationId": preview["operation_id"]}),
    );
    assert_eq!(operation["state"], "succeeded");
    let deployed = tmp
        .path()
        .join("data/setups")
        .join(created["id"].as_str().unwrap())
        .join("Mods/Tests.Example/Example.dll");
    assert_eq!(std::fs::read(&deployed).unwrap(), b"fixture");
    let mods = invoke(&window, "list_profile_mods", json!({}));
    assert_eq!(mods.as_array().unwrap().len(), 1);
    let details = invoke(
        &window,
        "get_mod_details",
        json!({"profileComponentId": mods[0]["profile_component_id"]}),
    );
    assert_eq!(details["name"], "Example");
    let removal = invoke(
        &window,
        "prepare_remove",
        json!({"profileComponentId": mods[0]["profile_component_id"]}),
    );
    invoke(
        &window,
        "execute_operation",
        json!({"operationId": removal["operation_id"]}),
    );
    assert!(invoke(&window, "list_profile_mods", json!({}))
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!deployed.exists());
    let cancelled = invoke(
        &window,
        "inspect_package_for_install",
        json!({"archivePath": archive}),
    );
    invoke(
        &window,
        "cancel_active_operation",
        json!({"operationId": cancelled["operation_id"]}),
    );
    assert_eq!(
        invoke(
            &window,
            "get_operation_details",
            json!({"operationId": cancelled["operation_id"]})
        )["state"],
        "cancelled"
    );
    let history = invoke(&window, "list_recent_operations", json!({"limit": 50}));
    assert!(history
        .as_array()
        .unwrap()
        .iter()
        .any(|op| op["id"] == operation["id"] && op["state"] == "succeeded"));
}

#[test]
fn startup_preserves_interrupted_mod_operation_evidence() {
    use manager_app::ports::repositories::OperationRepository;
    use manager_core::ids::{OperationId, ProfileId};
    use manager_core::operation::{Operation, OperationKind, OperationState};
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let id = OperationId::new();
    let profile = ProfileId::new();
    let evidence = paths
        .profile_staging_dir(&profile, &id)
        .join("recovery-evidence");
    {
        let state = AppState::new_with_paths(paths.clone()).unwrap();
        std::fs::create_dir_all(evidence.parent().unwrap()).unwrap();
        std::fs::write(&evidence, b"preserve me").unwrap();
        let time = "2026-09-13T00:00:00Z".parse().unwrap();
        state
            .repo
            .save_operation(&Operation {
                id,
                kind: OperationKind::ModInstall,
                state: OperationState::Running,
                game_installation_id: None,
                profile_id: Some(profile),
                expected_profile_revision: Some(1),
                plan_schema_version: 1,
                plan_json: "{}".into(),
                progress_current: None,
                progress_total: None,
                error_code: None,
                error_json: None,
                cancellation_requested: false,
                created_at: time,
                updated_at: time,
                completed_at: None,
            })
            .unwrap();
    }
    let restarted = AppState::new_with_paths(paths).unwrap();
    assert_eq!(std::fs::read(evidence).unwrap(), b"preserve me");
    assert_eq!(
        restarted.repo.get_operation(&id).unwrap().unwrap().state,
        OperationState::RecoveryRequired
    );
    assert!(restarted
        .services
        .bootstrap
        .get_bootstrap()
        .unwrap()
        .recovery_summary
        .is_some());
    assert!(
        restarted.recovery_error.lock().unwrap().is_none(),
        "The legacy recovery engine must not process modern operations"
    );
}
