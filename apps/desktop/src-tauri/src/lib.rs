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
            // Bootstrap
            bootstrap,
            set_onboarding_disposition,
            // Games
            discover_game_installations,
            inspect_game_path,
            accept_game,
            list_games,
            set_active_game,
            discover_games,
            choose_game,
            select_game,
            get_app_snapshot,
            // Profiles
            list_profiles,
            get_profile,
            get_active_profile,
            get_profile_overview,
            create_profile,
            select_profile,
            duplicate_profile,
            delete_profile,
            // Mods & Queries
            list_mods,
            get_mod_details,
            toggle_mod,
            prepare_install,
            prepare_remove,
            inspect_mod,
            install_mod,
            remove_mod,
            cancel_inspection,
            // Operations
            commit_operation,
            get_operation,
            list_operations,
            retry_recovery,
            cancel_operation,
            // SMAPI
            get_smapi_status,
            prepare_smapi,
            install_smapi,
            // Launch
            get_launch_preflight,
            launch_game,
            get_session,
            poll_session,
            terminate_game,
            // Diagnostics & Health
            get_health_summary,
            get_diagnostics,
            get_smapi_log,
            get_smapi_log_path,
            // Dialogs
            pick_mod_file,
            pick_game_directory,
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_tauri_context_embedded_assets_presence() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        assert!(
            context.assets.get(&"index.html".into()).is_some(),
            "Application context must have index.html embedded in assets. If empty, ensure 'custom-protocol' feature is active."
        );

        for win in &context.config().app.windows {
            if let tauri::utils::config::WebviewUrl::External(url) = &win.url {
                panic!(
                    "Window '{}' is configured with external dev url '{}' instead of embedded app assets",
                    win.label, url
                );
            }
        }
    }
}



