//! Commands for recovery notices, undoing a removal and repairing a folder
//! clash. Thin adapters, as in `commands.rs`.

use crate::events;
use crate::ipc::{self, IntoIpcResult, IpcResult};
use crate::state::AppState;
use manager_app::api::dto::{RecoveredOperationDto, RemovalUndoResultDto};
use manager_core::ids::{OperationId, ProfileComponentId};
use std::str::FromStr;
use tauri::State;

/// What startup recovery did, until it is dismissed.
#[tauri::command]
pub fn get_startup_recovery_report(
    state: State<'_, AppState>,
) -> IpcResult<Vec<RecoveredOperationDto>> {
    Ok(state
        .startup_recovery
        .lock()
        .map(|report| report.clone())
        .unwrap_or_default())
}

#[tauri::command]
pub fn dismiss_startup_recovery_report(state: State<'_, AppState>) -> IpcResult<()> {
    if let Ok(mut report) = state.startup_recovery.lock() {
        report.clear();
    }
    Ok(())
}

/// Installs a removed mod again from its stored archive, with the settings it
/// had when it was removed.
#[tauri::command]
pub fn undo_removal<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    operation_id: String,
) -> IpcResult<RemovalUndoResultDto> {
    let id = OperationId::from_str(&operation_id)
        .map_err(ipc::invalid_operation_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        manager_app::services::removal_undo::RemovalUndo::new(
            state.services.operations.clone(),
            crate::commands::reinstall_service(&state),
            std::sync::Arc::new(manager_infra::deployed_files::FilesystemDeployedFiles::new(
                state.paths.clone(),
            )),
        )
        .undo(&id)
        .into_ipc()
    })
}

/// The repair offered when another folder of the same name is in Mods: moves
/// that folder into the recovery folder, then enables or disables the mod.
#[tauri::command]
pub fn move_aside_and_set_mod_enabled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_component_id: String,
    enabled: bool,
) -> IpcResult<()> {
    let cid = ProfileComponentId::from_str(&profile_component_id)
        .map_err(ipc::invalid_profile_component_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state
            .services
            .toggle
            .move_aside_and_set_enabled(&cid, enabled)
            .into_ipc()
    })
}
