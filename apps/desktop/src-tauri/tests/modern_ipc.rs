use manager_app::ports::repositories::{DeploymentRepository, OperationRepository};
use manager_infra::paths::AppPaths;
use rusqlite::Connection;
#[cfg(target_os = "linux")]
use serde_json::Value;
use stardew_mod_manager::state::AppState;

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
#[test]
fn modern_onboarding_and_profile_commands_dispatch_through_production_handler() {
    use serde_json::json;
    use sha2::Digest;
    use stardew_mod_manager::configure;
    use std::io::Write;
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
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let release = manager_core::smapi::get_pinned_smapi_release();
    let installer_archive = paths
        .smapi_cache_dir()
        .join(format!("SMAPI-{}-installer.zip", release.version));
    std::fs::create_dir_all(installer_archive.parent().unwrap()).unwrap();
    let installer_file = std::fs::File::create(&installer_archive).unwrap();
    let mut installer_zip = zip::ZipWriter::new(installer_file);
    installer_zip
        .start_file(
            manager_core::smapi::PINNED_INSTALLER_INTERNAL_PATH,
            zip::write::SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
    installer_zip
        .write_all(b"#!/bin/sh\nwhile [ \"$#\" -gt 0 ]; do if [ \"$1\" = \"--game-path\" ]; then GAME_PATH=\"$2\"; shift 2; else shift; fi; done\nmkdir -p \"$GAME_PATH/smapi-internal\" \"$GAME_PATH/Mods/SaveBackup\"\nprintf '#!/bin/sh\\nsleep 30\\n' > \"$GAME_PATH/StardewModdingAPI\"\nchmod +x \"$GAME_PATH/StardewModdingAPI\"\ntouch \"$GAME_PATH/StardewModdingAPI.dll\"\nprintf '{\"targets\":{\".NETCoreApp,Version=v6.0/linux-x64\":{\"StardewModdingAPI/4.1.10\":{}}}}' > \"$GAME_PATH/StardewModdingAPI.deps.json\"\necho 'SMAPI is installed!'\n")
        .unwrap();
    installer_zip.finish().unwrap();
    let installer_bytes = std::fs::read(&installer_archive).unwrap();
    let mut installer_hasher = sha2::Sha256::new();
    installer_hasher.update(installer_bytes);
    let installer_hash = manager_core::ids::hash_to_hex(installer_hasher.finalize());
    let state = AppState::new_with_expected_smapi_hash(paths, Some(&installer_hash)).unwrap();
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
    let smapi = invoke(
        &window,
        "install_pinned_smapi",
        json!({"gameId": registered["id"]}),
    );
    assert_eq!(smapi["is_installed"], true);
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
    let launch = invoke(
        &window,
        "launch_active_profile",
        json!({"mode": "modded", "profileId": created["id"]}),
    );
    assert!(launch["id"].is_string());
    invoke(
        &window,
        "terminate_active_launch_session",
        json!({"sessionId": launch["id"]}),
    );
    assert!(invoke(&window, "get_active_launch_session", json!({})).is_null());
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
    assert_eq!(smapi["is_installed"], true);
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
        let state = AppState::new_with_expected_smapi_hash(paths.clone(), Some("test")).unwrap();
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
    let restarted = AppState::new_with_expected_smapi_hash(paths, Some("test")).unwrap();
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
}

#[test]
fn modern_startup_reconciles_v1_interrupted_install_and_remove_operations() {
    use manager_core::ids::derive_uuid;
    use serde_json::json;

    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    let cache_dir = tmp.path().join("cache");
    std::fs::create_dir_all(data_dir.join("setups/setup-1/Mods")).unwrap();
    let game_dir = tmp.path().join("game");
    std::fs::create_dir_all(&game_dir).unwrap();

    let conn = Connection::open(data_dir.join("state.sqlite3")).unwrap();
    let migration = include_str!("../../../../crates/manager-infra/migrations/0001_initial.sql");
    conn.execute_batch(&format!(
        "BEGIN;\nCREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);\n{}\nINSERT INTO schema_migrations (version, applied_at) VALUES (1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));\nCOMMIT;",
        migration
    ))
    .unwrap();
    conn.execute(
        "INSERT INTO game_installations (id, canonical_root, platform_kind, detected_version, validated_at, is_fresh)
         VALUES ('game-1', ?1, 'steam_native', '1.6.8', '2026-01-01T00:00:00Z', 1)",
        [&game_dir.to_string_lossy().to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO setups (id, game_id, display_name, relative_mods_dir, created_at)
         VALUES ('setup-1', 'game-1', 'Default Setup', 'Mods', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO packages (hash, original_filename, source_kind, byte_size, created_at)
         VALUES ('hash-abc', 'mod.zip', 'local_zip', 123, '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO installed_mods (id, setup_id, package_id, unique_id, name, author, version, description, raw_manifest, relative_target_path, file_inventory_json, installed_at)
         VALUES ('mod-1', 'setup-1', 'hash-abc', 'Author.Mod', 'Legacy Mod', 'Author', '1.0.0', NULL, '{}', 'LegacyRemove', '[\"evidence.txt\"]', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    let install_operation = "op-install-recovery";
    let install_folder = "LegacyInstall";
    let install_live = data_dir.join("setups/setup-1/Mods").join(install_folder);
    std::fs::create_dir_all(&install_live).unwrap();
    std::fs::write(
        install_live.join("evidence.txt"),
        b"published before DB commit",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO operations (id, kind, state, plan_json, created_at, updated_at, schema_version)
         VALUES (?1, 'mod_install', 'recovering', ?2, '2026-01-01T00:00:01Z', '2026-01-01T00:00:01Z', 1)",
        rusqlite::params![
            install_operation,
            json!({
                "plan_id": "plan-install-recovery",
                "setup_id": "setup-1",
                "package_hash": "hash-abc",
                "mod_folder_name": install_folder,
            })
            .to_string()
        ],
    )
    .unwrap();

    let remove_operation = "op-remove-recovery";
    let remove_folder = "LegacyRemove";
    let remove_recovery = data_dir
        .join("setups/setup-1/.recovery")
        .join(remove_operation)
        .join(remove_folder);
    std::fs::create_dir_all(&remove_recovery).unwrap();
    std::fs::write(
        remove_recovery.join("evidence.txt"),
        b"removed before DB commit",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO operations (id, kind, state, plan_json, created_at, updated_at, schema_version)
         VALUES (?1, 'mod_remove', 'recovering', ?2, '2026-01-01T00:00:02Z', '2026-01-01T00:00:02Z', 1)",
        rusqlite::params![
            remove_operation,
            json!({
                "operation_id": remove_operation,
                "setup_id": "setup-1",
                "installed_mod_id": "mod-1",
                "mod_unique_id": "Author.Mod",
                "relative_folder_path": remove_folder,
                "recovery_folder_path": remove_recovery,
                "bundle_mod_ids": ["mod-1"],
            })
            .to_string()
        ],
    )
    .unwrap();
    drop(conn);

    let paths = AppPaths::new(data_dir.clone(), cache_dir);
    let state = AppState::new_with_expected_smapi_hash(paths.clone(), Some("test")).unwrap();
    state.services.operations.retry_recovery().unwrap();

    let install_id = manager_core::ids::OperationId::from_uuid(derive_uuid(install_operation));
    let remove_id = manager_core::ids::OperationId::from_uuid(derive_uuid(remove_operation));
    assert_eq!(
        state
            .repo
            .get_operation(&install_id)
            .unwrap()
            .unwrap()
            .state,
        manager_core::operation::OperationState::Failed
    );
    assert_eq!(
        state.repo.get_operation(&remove_id).unwrap().unwrap().state,
        manager_core::operation::OperationState::Succeeded
    );
    let remove_record = state.repo.get_operation(&remove_id).unwrap().unwrap();
    assert_eq!(
        remove_record.error_code.as_deref(),
        Some("LEGACY_REMOVAL_RECONCILED")
    );
    assert!(remove_record
        .error_json
        .as_deref()
        .unwrap()
        .contains("filesystem and database state were reconciled"));

    let profile_id = manager_core::ids::ProfileId::from_uuid(derive_uuid("setup-1"));
    assert!(!paths
        .profile_mods_dir(&profile_id)
        .join(install_folder)
        .exists());
    assert!(!paths
        .profile_mods_dir(&profile_id)
        .join(remove_folder)
        .exists());
    assert!(paths
        .profile_recovery_dir(&profile_id, &install_id)
        .join(install_folder)
        .join("evidence.txt")
        .exists());
    assert!(paths
        .profile_recovery_dir(&profile_id, &remove_id)
        .join(remove_folder)
        .join("evidence.txt")
        .exists());
    assert!(state
        .repo
        .list_profile_components(&profile_id)
        .unwrap()
        .is_empty());
    assert!(state
        .services
        .bootstrap
        .get_bootstrap()
        .unwrap()
        .recovery_summary
        .is_none());
}

#[test]
fn legacy_install_quarantine_failure_retains_recovery_required() {
    use manager_app::ports::repositories::{
        GameInstallationRepository, OperationRepository, ProfileRepository,
    };
    use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
    use manager_core::ids::{GameInstallationId, OperationId};
    use manager_core::operation::{Operation, OperationKind, OperationState};
    use manager_core::profile::Profile;
    use serde_json::json;

    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let state = AppState::new_with_expected_smapi_hash(paths.clone(), Some("test")).unwrap();
    let timestamp = "2026-09-13T00:00:00Z".parse().unwrap();

    let game_id = GameInstallationId::new();
    state
        .repo
        .save_game(&GameInstallation {
            id: game_id,
            canonical_root: tmp.path().join("game"),
            operating_system: OperatingSystem::Linux,
            storefront: Storefront::Steam,
            management_mode: ManagementMode::Managed,
            created_at: timestamp,
        })
        .unwrap();
    let profile = Profile::new(game_id, "Legacy recovery");
    let profile_id = profile.id;
    state.repo.save_profile(&profile).unwrap();

    let operation_id = OperationId::new();
    let folder = "LegacyInstall";
    let live = paths.profile_mods_dir(&profile_id).join(folder);
    std::fs::create_dir_all(&live).unwrap();
    std::fs::write(live.join("evidence.txt"), b"unowned live folder").unwrap();
    let recovery_root = paths.profile_recovery_dir(&profile_id, &operation_id);
    std::fs::create_dir_all(recovery_root.parent().unwrap()).unwrap();
    std::fs::write(&recovery_root, b"blocks quarantine").unwrap();

    let now = timestamp;
    state
        .repo
        .save_operation(&Operation {
            id: operation_id,
            kind: OperationKind::ModInstall,
            state: OperationState::RecoveryRequired,
            game_installation_id: Some(game_id),
            profile_id: Some(profile_id),
            expected_profile_revision: Some(profile.revision),
            plan_schema_version: 1,
            plan_json: json!({"mod_folder_name": folder}).to_string(),
            progress_current: None,
            progress_total: None,
            error_code: Some("LEGACY_OPERATION_REQUIRES_RECONCILIATION".into()),
            error_json: None,
            cancellation_requested: false,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
        .unwrap();

    assert!(state.services.operations.retry_recovery().is_err());
    let operation = state.repo.get_operation(&operation_id).unwrap().unwrap();
    assert_eq!(operation.state, OperationState::RecoveryRequired);
    assert_eq!(
        operation.error_code.as_deref(),
        Some("LEGACY_INSTALL_RECONCILIATION_FAILED")
    );
    assert!(operation
        .error_json
        .as_deref()
        .unwrap()
        .contains("Failed to create recovery directory"));
    assert!(live.join("evidence.txt").exists());
    assert!(recovery_root.is_file());
}

#[test]
fn legacy_removal_without_files_retains_recovery_required() {
    use manager_app::ports::repositories::{
        DeploymentRepository, GameInstallationRepository, OperationRepository,
        PackageCatalogRepository, ProfileRepository,
    };
    use manager_core::deployment::{
        DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
    };
    use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
    use manager_core::ids::{
        ArtifactHash, DeploymentId, GameInstallationId, OperationId, PackageComponentId,
        ProfileComponentId,
    };
    use manager_core::manifest::{Manifest, ModDependency};
    use manager_core::operation::{Operation, OperationKind, OperationState};
    use manager_core::package::{PackageArtifact, PackageComponent};
    use manager_core::profile::Profile;
    use serde_json::json;

    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let state = AppState::new_with_expected_smapi_hash(paths.clone(), Some("test")).unwrap();
    let timestamp = "2026-09-13T00:00:00Z".parse().unwrap();

    let game_id = GameInstallationId::new();
    state
        .repo
        .save_game(&GameInstallation {
            id: game_id,
            canonical_root: tmp.path().join("game"),
            operating_system: OperatingSystem::Linux,
            storefront: Storefront::Steam,
            management_mode: ManagementMode::Managed,
            created_at: timestamp,
        })
        .unwrap();
    let profile = Profile::new(game_id, "Legacy removal");
    let profile_id = profile.id;
    state.repo.save_profile(&profile).unwrap();

    let artifact_hash = ArtifactHash::new("a".repeat(64));
    state
        .repo
        .save_artifact(&PackageArtifact {
            hash: artifact_hash.clone(),
            byte_size: 1,
            storage_relative_path: "packages/test.zip".into(),
            first_seen_at: timestamp,
        })
        .unwrap();
    let package_component_id = PackageComponentId::new();
    state
        .repo
        .save_package_component(&PackageComponent {
            id: package_component_id,
            artifact_hash: artifact_hash.clone(),
            unique_id: "Tests.Legacy".into(),
            name: "Legacy".into(),
            author: "Tests".into(),
            version: "1.0.0".into(),
            description: None,
            relative_component_root: "".into(),
            raw_manifest: "{}".into(),
            manifest: Manifest {
                unique_id: "Tests.Legacy".into(),
                name: "Legacy".into(),
                author: "Tests".into(),
                version: "1.0.0".into(),
                description: None,
                entry_dll: None,
                minimum_api_version: None,
                minimum_game_version: None,
                update_keys: Vec::new(),
                dependencies: Vec::<ModDependency>::new(),
                content_pack_for: None,
            },
        })
        .unwrap();

    let deployment_id = DeploymentId::new();
    let component_id = ProfileComponentId::new();
    state
        .repo
        .save_deployment(&ProfileDeployment {
            id: deployment_id,
            profile_id,
            artifact_hash,
            root_relative_path: "LegacyRemove".into(),
            installed_at: timestamp,
            state: DeploymentState::Present,
        })
        .unwrap();
    state
        .repo
        .save_profile_component(&ProfileComponent {
            id: component_id,
            profile_id,
            deployment_id,
            package_component_id,
            enabled: true,
            installed_reason: InstalledReason::Direct,
        })
        .unwrap();

    let operation_id = OperationId::new();
    let now = timestamp;
    state
        .repo
        .save_operation(&Operation {
            id: operation_id,
            kind: OperationKind::ModRemove,
            state: OperationState::RecoveryRequired,
            game_installation_id: Some(game_id),
            profile_id: Some(profile_id),
            expected_profile_revision: Some(profile.revision),
            plan_schema_version: 1,
            plan_json: json!({
                "deployment_rel_path": "LegacyRemove",
                "bundle_mod_ids": [component_id.to_string()]
            })
            .to_string(),
            progress_current: None,
            progress_total: None,
            error_code: Some("LEGACY_OPERATION_REQUIRES_RECONCILIATION".into()),
            error_json: None,
            cancellation_requested: false,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
        .unwrap();

    assert!(state.services.operations.retry_recovery().is_err());
    let operation = state.repo.get_operation(&operation_id).unwrap().unwrap();
    assert_eq!(operation.state, OperationState::RecoveryRequired);
    assert_eq!(
        operation.error_code.as_deref(),
        Some("LEGACY_REMOVAL_EVIDENCE_MISSING")
    );
    assert!(operation
        .error_json
        .as_deref()
        .unwrap()
        .contains("missing from both live and recovery storage"));
    assert!(state.repo.get_deployment(&deployment_id).unwrap().is_some());
    assert_eq!(
        state
            .repo
            .list_profile_components(&profile_id)
            .unwrap()
            .len(),
        1
    );
    assert!(!paths
        .profile_mods_dir(&profile_id)
        .join("LegacyRemove")
        .exists());
    assert!(!paths
        .profile_recovery_dir(&profile_id, &operation_id)
        .join("LegacyRemove")
        .exists());
}
