// Application errors are intentionally rich (code, category, summary,
// recoverability, operation id), so command results carry a larger Err variant
// than clippy's default threshold. The size is the contract, not an accident.
#![allow(clippy::result_large_err)]

pub mod commands;
pub mod ipc;
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
            validate_game_installation_path,
            register_game_installation,
            list_game_installations,
            // Profiles
            list_profiles,
            list_archived_profiles,
            restore_profile,
            create_profile,
            activate_profile,
            archive_profile,
            get_active_profile_overview,
            // Mods & Queries
            list_profile_mods,
            get_mod_details,
            inspect_package_for_install,
            prepare_remove,
            // Operations
            execute_operation,
            get_operation_details,
            list_recent_operations,
            retry_recovery,
            cancel_active_operation,
            // SMAPI
            get_smapi_status,
            modern_smapi::install_pinned_smapi,
            // Launch
            launch_active_profile,
            get_active_launch_session,
            terminate_active_launch_session,
            // Diagnostics
            get_diagnostics_report,
            // Dialogs
            pick_folder_dialog,
            pick_archive_dialog,
        ])
        .setup(|app| {
            if let Some(main_window) = app.get_webview_window("main") {
                let state = app.state::<AppState>();
                restore_window_geometry(&main_window, state.repo.as_ref());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Moved(_) | WindowEvent::Resized(_) = event {
                let app = window.app_handle();
                if let Some(state) = app.try_state::<AppState>() {
                    if let Some(main_window) = app.get_webview_window("main") {
                        persist_window_geometry(&main_window, state.repo.as_ref());
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

    /// Extracts the Tauri command names a TypeScript line invokes through the
    /// shared IPC wrapper, for example invokeApi<Foo>("list_profiles").
    fn invoked_command_names(line: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut search = line;
        while let Some(index) = search.find("invoke") {
            let rest = &search[index + "invoke".len()..];
            search = rest;
            let rest = rest.strip_prefix("Api").unwrap_or(rest);
            let arguments = if let Some(generics) = rest.strip_prefix('<') {
                match generics.find(">(") {
                    Some(end) => &generics[end + 2..],
                    None => continue,
                }
            } else if let Some(arguments) = rest.strip_prefix('(') {
                arguments
            } else {
                continue;
            };
            if let Some(quoted) = arguments.strip_prefix('"') {
                if let Some(end) = quoted.find('"') {
                    names.push(quoted[..end].to_string());
                }
            }
        }
        names
    }

    #[test]
    fn test_all_frontend_invokes_are_registered_tauri_commands() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

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

        let mut inspected = 0usize;
        for relative in [
            "../src/shared/api/client.ts",
            "../src/shared/api/onboarding.ts",
        ] {
            let source = std::fs::read_to_string(manifest_dir.join(relative))
                .unwrap_or_else(|_| panic!("Could not read {relative}"));
            for line in source.lines() {
                for cmd_name in invoked_command_names(line) {
                    inspected += 1;
                    assert!(
                        registered_cmds.contains(cmd_name.as_str()),
                        "Frontend {relative} calls \"{cmd_name}\", but it is not registered in tauri::generate_handler![...] in lib.rs!"
                    );
                }
            }
        }
        assert!(
            inspected > 0,
            "No frontend IPC invocations were found; the command-registration guard is not scanning any calls."
        );
    }
}
