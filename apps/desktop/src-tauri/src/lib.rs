pub mod commands;
pub mod modern_smapi;
pub mod state;
pub mod window;

use commands::*;
use state::AppState;
use tauri::{Manager, WindowEvent};
use window::{persist_window_geometry, restore_window_geometry};

pub fn configure<R: tauri::Runtime>(
    builder: tauri::Builder<R>,
    app_state: AppState,
) -> tauri::Builder<R> {
    builder
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
            archive_profile,
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
            pick_folder_dialog,
            pick_archive_dialog,
            // Modern frontend API bridges
            list_game_installations,
            register_game_installation,
            validate_game_installation_path,
            activate_profile,
            get_active_profile_overview,
            list_profile_mods,
            toggle_mod_enabled,
            inspect_package_for_install,
            execute_operation,
            list_recent_operations,
            get_operation_details,
            cancel_active_operation,
            modern_smapi::install_pinned_smapi,
            launch_active_profile,
            get_active_launch_session,
            terminate_active_launch_session,
            get_diagnostics_report,
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
}

pub fn run() {
    let app_state = AppState::new().expect("Failed to initialize application state");
    configure(tauri::Builder::default(), app_state)
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

    #[test]
    fn test_all_frontend_invokes_are_registered_tauri_commands() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let client_ts_path = manifest_dir.join("../src/shared/api/client.ts");
        let client_ts = std::fs::read_to_string(&client_ts_path).expect("Could not read client.ts");

        let lib_rs_path = manifest_dir.join("src/lib.rs");
        let lib_rs = std::fs::read_to_string(&lib_rs_path).expect("Could not read lib.rs");

        let handler_start = lib_rs
            .find("generate_handler![")
            .expect("Missing generate_handler!");
        let handler_end = lib_rs[handler_start..]
            .find(']')
            .expect("Missing closing bracket for generate_handler!");
        let handler_block = &lib_rs[handler_start..handler_start + handler_end];

        let registered_cmds: std::collections::HashSet<&str> = handler_block
            .lines()
            .map(|l| l.trim().trim_end_matches(','))
            .filter(|l| {
                !l.is_empty() && !l.starts_with("//") && !l.starts_with("generate_handler!")
            })
            .map(|l| l.rsplit("::").next().unwrap_or(l))
            .collect();

        for line in client_ts.lines() {
            if let Some(idx) = line.find("invoke(\"") {
                let rest = &line[idx + 8..];
                if let Some(end_quote) = rest.find('"') {
                    let cmd_name = &rest[..end_quote];
                    assert!(
                        registered_cmds.contains(cmd_name),
                        "Frontend client.ts calls invoke(\"{}\"), but \"{}\" is not registered in tauri::generate_handler![...] in lib.rs!",
                        cmd_name, cmd_name
                    );
                }
            }
        }
    }
}
