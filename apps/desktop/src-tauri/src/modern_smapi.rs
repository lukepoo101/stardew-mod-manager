use crate::ipc::{self, IntoIpcResult, IpcResult};
use crate::state::AppState;
use manager_app::api::dto::SmapiStatusDto;
use manager_core::ids::GameInstallationId;
use std::str::FromStr;
use tauri::State;

#[tauri::command]
pub async fn install_pinned_smapi(
    state: State<'_, AppState>,
    game_installation_id: Option<String>,
    game_id: Option<String>,
) -> IpcResult<SmapiStatusDto> {
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
    services.smapi.install_smapi(&gid).await.into_ipc()?;
    services.smapi.get_smapi_status(&gid).into_ipc()
}
