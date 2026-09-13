pub mod commands;
pub mod state;
pub mod window;

use commands::*;
use state::AppState;
use tauri::{Manager, WindowEvent};
use window::{persist_window_geometry, restore_window_geometry};

pub fn run() {
    let app_state = AppState::new().expect("Failed to initialize application state");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            retry_recovery,
            cancel_inspection,
            discover_games,
            choose_game,
            select_game,
            prepare_smapi,
            install_smapi,
            pick_mod_file,
            inspect_mod,
            install_mod,
            remove_mod,
            launch_game,
            get_operation,
            cancel_operation,
            get_session,
            poll_session,
            terminate_game,
            get_smapi_log,
            get_smapi_log_path,
        ])
        .setup(|app| {
            if let Some(main_window) = app.get_webview_window("main") {
                let state = app.state::<AppState>();
                restore_window_geometry(&main_window, &state.use_cases.repo);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Moved(_) | WindowEvent::Resized(_) = event {
                let app = window.app_handle();
                if let Some(state) = app.try_state::<AppState>() {
                    if let Some(main_window) = app.get_webview_window("main") {
                        persist_window_geometry(&main_window, &state.use_cases.repo);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
