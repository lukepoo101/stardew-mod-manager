use crate::background;
use crate::events;
use crate::ipc::{self, IntoIpcResult, IpcResult};
use crate::state::AppState;
use manager_app::api::dto::{SetupPreviewDto, SmapiStatusDto};
use manager_core::ids::GameInstallationId;
use std::str::FromStr;
use tauri::Manager;

#[tauri::command]
pub async fn install_pinned_smapi<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    game_installation_id: Option<String>,
    game_id: Option<String>,
    previewed_version: Option<String>,
    version: Option<String>,
    allow_unverified: Option<bool>,
) -> IpcResult<SmapiStatusDto> {
    let handle = app.clone();
    background::change_async(app, async move {
        let app = handle;
        let state = app.state::<AppState>();
        let allow_unverified = allow_unverified.unwrap_or(false);
        let services = state.services.clone();
        let gid_str = match game_installation_id.or(game_id) {
            Some(gid) => gid,
            None => services
                .bootstrap
                .get_bootstrap()
                .into_ipc()?
                .active_game_installation_id
                .ok_or_else(ipc::no_active_game)
                .into_ipc()?,
        };

        let gid = GameInstallationId::from_str(&gid_str)
            .map_err(ipc::invalid_game_installation_id)
            .into_ipc()?;
        // Emit after the install attempt rather than only after success: a failed
        // SMAPI setup can still have changed the game directory or persisted state.
        // When the user reviewed a preview, setup runs only if that plan still
        // holds and every location it needs is still usable.
        let install_result = match previewed_version {
            Some(version) => {
                let preview = services
                    .smapi
                    .preview_setup(
                        &gid,
                        &manager_locations(&state),
                        &manager_infra::access_probe::probe_for_smapi_setup,
                        Some(&version),
                    )
                    .into_ipc()?;
                // The game folder is inspected again right before the installer
                // runs, so a folder that changed since it was chosen is caught.
                let inspection = services
                    .games
                    .inspect_path(std::path::Path::new(&preview.game_path), None)
                    .into_ipc()?;
                if !inspection.is_usable {
                    return Err::<SmapiStatusDto, _>(manager_app::error::AppError::validation(
                        "GAME_NOT_READY",
                        format!(
                            "The game folder is no longer ready for setup ({}). {}",
                            inspection.support_state,
                            inspection.evidence.join("; ")
                        ),
                    ))
                    .into_ipc();
                }
                if !preview.can_proceed {
                    return Err::<SmapiStatusDto, _>(manager_app::error::AppError::validation(
                        "SETUP_CHECKS_FAILED",
                        "A location setup needs cannot be used. Review the setup checks.",
                    ))
                    .into_ipc();
                }
                services
                    .smapi
                    .install_smapi_as_previewed(&gid, &version, allow_unverified)
                    .await
                    .into_ipc()
            }
            None => services
                .smapi
                .install_release(&gid, version.as_deref(), allow_unverified)
                .await
                .into_ipc(),
        };
        events::emit_backend_state_changed(&app);
        install_result?;
        services.smapi.get_smapi_status(&gid).into_ipc()
    })
    .await
}

fn manager_locations(state: &AppState) -> Vec<(String, std::path::PathBuf)> {
    vec![
        (
            "Manager data (database, stored mods, each profile's Mods folder)".to_string(),
            state.paths.data_dir().to_path_buf(),
        ),
        (
            "Manager cache (downloads)".to_string(),
            state.paths.cache_dir().to_path_buf(),
        ),
    ]
}

/// What SMAPI setup will change and whether it can run, without changing
/// anything.
#[tauri::command]
pub async fn preview_smapi_setup<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    game_installation_id: String,
    version: Option<String>,
) -> IpcResult<SetupPreviewDto> {
    background::run(app, move |_app, state| {
        let gid = GameInstallationId::from_str(&game_installation_id)
            .map_err(ipc::invalid_game_installation_id)
            .into_ipc()?;
        state
            .services
            .smapi
            .preview_setup(
                &gid,
                &manager_locations(&state),
                &manager_infra::access_probe::probe_for_smapi_setup,
                version.as_deref(),
            )
            .into_ipc()
    })
    .await
}

/// Every SMAPI release the manager can install, with how each fits this
/// game. The list is read from SMAPI's published sources at most once a day,
/// or now when `refresh`.
#[tauri::command]
pub async fn get_smapi_catalog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    game_installation_id: String,
    refresh: Option<bool>,
) -> IpcResult<manager_app::api::dto::SmapiCatalogDto> {
    background::contained_task(async move {
        let gid = GameInstallationId::from_str(&game_installation_id)
            .map_err(ipc::invalid_game_installation_id)
            .into_ipc()?;
        let smapi = app.state::<AppState>().services.smapi.clone();
        let view = smapi
            .catalog_view(&gid, refresh.unwrap_or(false))
            .await
            .into_ipc()?;
        // A list just read from SMAPI can change what is suggested and
        // whether an update is available, so views showing SMAPI's status
        // are refreshed.
        if view.source == "online" {
            events::emit_backend_state_changed(&app);
        }
        Ok(view)
    })
    .await
}

#[tauri::command]
pub async fn get_smapi_update_settings<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> IpcResult<manager_app::api::dto::SmapiUpdateSettingsDto> {
    background::run(app, move |_app, state| {
        Ok(state
            .services
            .smapi
            .catalog()
            .map(|c| c.update_settings())
            .unwrap_or_default())
    })
    .await
}

#[tauri::command]
pub async fn set_smapi_update_settings<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    settings: manager_app::api::dto::SmapiUpdateSettingsDto,
) -> IpcResult<()> {
    background::change(app, move |_app, state| {
        match state.services.smapi.catalog() {
            Some(catalog) => catalog.set_update_settings(&settings).into_ipc(),
            None => Ok(()),
        }
    })
    .await
}

/// Removes SMAPI from a game folder with the upstream uninstaller. Mods and
/// profiles are kept.
#[tauri::command]
pub async fn uninstall_smapi<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    game_installation_id: String,
) -> IpcResult<SmapiStatusDto> {
    let handle = app.clone();
    background::change_async(app, async move {
        let services = handle.state::<AppState>().services.clone();
        let gid = GameInstallationId::from_str(&game_installation_id)
            .map_err(ipc::invalid_game_installation_id)
            .into_ipc()?;
        let result = services.smapi.uninstall_smapi(&gid).await.into_ipc();
        events::emit_backend_state_changed(&handle);
        result?;
        services.smapi.get_smapi_status(&gid).into_ipc()
    })
    .await
}
