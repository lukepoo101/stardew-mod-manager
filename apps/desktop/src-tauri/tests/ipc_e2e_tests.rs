use manager_infra::paths::AppPaths;
use stardew_mod_manager::commands;
use stardew_mod_manager::state::AppState;
use std::fs::File;
use std::io::Write;
use tauri::Manager;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn create_mock_mod_zip(path: &std::path::Path) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    let manifest = r#"{
        "Name": "IPC Test Mod",
        "Author": "E2ETester",
        "Version": "1.0.0",
        "UniqueID": "E2ETester.IPCTestMod",
        "EntryDll": "IPCTestMod.dll"
    }"#;

    zip.start_file("IPCTestMod/manifest.json", options).unwrap();
    zip.write_all(manifest.as_bytes()).unwrap();
    zip.start_file("IPCTestMod/IPCTestMod.dll", options)
        .unwrap();
    zip.write_all(b"fake dll binary content").unwrap();
    zip.finish().unwrap();
}

use sha2::{Digest, Sha256};

#[test]
fn test_tauri_ipc_full_lifecycle_e2e() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // 1. Setup isolated AppPaths
    let data_dir = root.join("data");
    let cache_dir = root.join("cache");
    let paths = AppPaths::new(data_dir.clone(), cache_dir.clone());

    // 2. Setup mock game installation directory
    let game_dir = root.join("StardewValley");
    std::fs::create_dir_all(&game_dir).unwrap();
    File::create(game_dir.join("Stardew Valley.dll")).unwrap();
    std::fs::write(game_dir.join("StardewValley"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(
        game_dir.join("Stardew Valley.deps.json"),
        r#"{"targets":{".NETCoreApp,Version=v6.0/linux-x64":{"Stardew Valley/1.6.15.24356":{}}}}"#,
    )
    .unwrap();

    // Mock SMAPI installer archive
    let installer_zip = paths.smapi_cache_dir().join("SMAPI-4.1.10-installer.zip");
    std::fs::create_dir_all(installer_zip.parent().unwrap()).unwrap();
    {
        let file = File::create(&installer_zip).unwrap();
        let mut zip = ZipWriter::new(file);
        let opt = SimpleFileOptions::default().unix_permissions(0o755);
        zip.start_file("SMAPI 4.1.10 installer/internal/linux/SMAPI.Installer", opt)
            .unwrap();
        let script = r#"#!/bin/bash
while [[ $# -gt 0 ]]; do
  case $1 in
    --game-path)
      GAME_PATH="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [ -n "$GAME_PATH" ]; then
  mkdir -p "$GAME_PATH/smapi-internal"
  mkdir -p "$GAME_PATH/Mods/SaveBackup"
  printf '#!/bin/sh\nsleep 30\n' > "$GAME_PATH/StardewModdingAPI"
  chmod +x "$GAME_PATH/StardewModdingAPI"
  touch "$GAME_PATH/StardewModdingAPI.dll"
  touch "$GAME_PATH/StardewModdingAPI.deps.json"
  echo "SMAPI is installed!"
  exit 0
else
  echo "No --game-path provided"
  exit 1
fi
"#;
        zip.write_all(script.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    let installer_bytes = std::fs::read(&installer_zip).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&installer_bytes);
    let installer_hash = format!("{:x}", hasher.finalize());

    let app_state = AppState::new_with_expected_smapi_hash(paths.clone(), Some(&installer_hash))
        .expect("Failed to init AppState");

    let app = tauri::test::mock_app();
    let handle = app.handle();
    handle.manage(app_state);
    let state = handle.state::<AppState>();

    // --- TEST IPC COMMAND: get_app_snapshot (initial empty state) ---
    let initial_snap = commands::get_app_snapshot(state.clone(), None).unwrap();
    assert!(initial_snap.selected_game.is_none());
    assert!(!initial_snap.smapi_installed);

    // --- TEST IPC COMMAND: choose_game ---
    let game_inst =
        commands::choose_game(state.clone(), game_dir.to_string_lossy().to_string()).unwrap();
    assert_eq!(game_inst.canonical_root, game_dir.canonicalize().unwrap());
    assert!(game_inst.is_fresh);

    // --- TEST IPC COMMAND: select_game ---
    let select_snap = commands::select_game(
        state.clone(),
        Some(game_dir.to_string_lossy().to_string()),
        None,
        Some("manual_folder".to_string()),
    )
    .unwrap();
    assert!(select_snap.selected_game.is_some());
    assert!(select_snap.setup.is_some());
    let setup = select_snap.setup.unwrap();

    // --- TEST IPC COMMAND: prepare_smapi ---
    let release_info = commands::prepare_smapi().unwrap();
    assert_eq!(release_info.version, "4.1.10");

    // --- TEST IPC COMMAND: install_smapi ---
    let smapi_rec = commands::install_smapi(state.clone(), game_inst.id.clone()).unwrap();
    assert_eq!(smapi_rec.release_version, "4.1.10");

    // Snapshot should now show SMAPI installed
    let post_smapi_snap =
        commands::get_app_snapshot(state.clone(), Some(game_inst.id.clone())).unwrap();
    assert!(post_smapi_snap.smapi_installed);

    // --- TEST IPC COMMAND: inspect_mod ---
    let mod_zip = root.join("IPCTestMod.zip");
    create_mock_mod_zip(&mod_zip);
    let inspection = commands::inspect_mod(
        state.clone(),
        mod_zip.to_string_lossy().to_string(),
        setup.id.clone(),
    )
    .unwrap();
    assert_eq!(inspection.plan.manifest.unique_id, "E2ETester.IPCTestMod");
    assert!(inspection.plan.dependency_report.is_installable);

    // --- TEST IPC COMMAND: install_mod ---
    let installed = commands::install_mod(state.clone(), inspection.plan.plan_id.clone()).unwrap();
    assert_eq!(installed.unique_id, "E2ETester.IPCTestMod");

    // Post-install snapshot should list 1 mod
    let post_mod_snap =
        commands::get_app_snapshot(state.clone(), Some(game_inst.id.clone())).unwrap();
    assert_eq!(post_mod_snap.installed_mods.len(), 1);

    // --- TEST IPC COMMAND: launch_game ---
    let session =
        commands::launch_game(state.clone(), game_inst.id.clone(), setup.id.clone()).unwrap();
    assert!(session.pid.is_some());

    // --- TEST IPC COMMAND: poll_session ---
    let polled = commands::poll_session(state.clone(), session.id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(polled.id, session.id);

    // --- TEST IPC COMMAND: get_smapi_log & get_smapi_log_path ---
    let log_path_str = commands::get_smapi_log_path(state.clone()).unwrap();
    assert!(!log_path_str.is_empty());
    let _log_content = commands::get_smapi_log(state.clone()).unwrap();

    // --- TEST IPC COMMAND: terminate_game ---
    commands::terminate_game(state.clone(), Some(session.id.clone())).unwrap();

    // Poll should report exited
    let exited = commands::poll_session(state.clone(), session.id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(exited.state, manager_core::domain::SessionState::Exited);

    // --- TEST IPC COMMAND: remove_mod ---
    commands::remove_mod(state.clone(), installed.id.clone(), setup.id.clone()).unwrap();

    // Snapshot should now show 0 mods
    let final_snap = commands::get_app_snapshot(state.clone(), Some(game_inst.id.clone())).unwrap();
    assert_eq!(final_snap.installed_mods.len(), 0);
}

#[test]
fn test_tauri_ipc_error_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let data_dir = root.join("data");
    let cache_dir = root.join("cache");
    let paths = AppPaths::new(data_dir, cache_dir);
    let app_state = AppState::new_with_paths(paths).expect("Failed to init AppState");

    let app = tauri::test::mock_app();
    let handle = app.handle();
    handle.manage(app_state);
    let state = handle.state::<AppState>();

    // 1. choose_game on non-existent path
    let invalid_path = root.join("does_not_exist");
    let res = commands::choose_game(state.clone(), invalid_path.to_string_lossy().to_string());
    assert!(res.is_err());

    // 2. select_game with no candidates or paths
    let res2 = commands::select_game(state.clone(), None, None, None);
    assert!(res2.is_err());

    // 3. inspect_mod with nonexistent file
    let fake_mod_path = root.join("missing.zip");
    let res3 = commands::inspect_mod(
        state.clone(),
        fake_mod_path.to_string_lossy().to_string(),
        "setup_123".to_string(),
    );
    assert!(res3.is_err());

    // 4. terminate_game when no game is running succeeds idempotently
    let res4 = commands::terminate_game(state.clone(), None);
    assert!(res4.is_ok());

    // 5. remove_mod with nonexistent mod_id errors out
    let res5 = commands::remove_mod(
        state.clone(),
        "nonexistent_mod_123".to_string(),
        "setup_123".to_string(),
    );
    assert!(res5.is_err());
    assert!(res5.unwrap_err().contains("not found"));
}
