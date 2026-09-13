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
) -> Result<SmapiStatusDto, String> {
    let services = state.services.clone();
    let gid_str = if let Some(gid) = game_installation_id.or(game_id) {
        gid
    } else {
        services
            .bootstrap
            .get_bootstrap()
            .map_err(|e| e.to_string())?
            .active_game_installation_id
            .ok_or_else(|| "No active game".to_string())?
    };

    let gid = GameInstallationId::from_str(&gid_str).map_err(|e| e.to_string())?;
    services
        .smapi
        .install_smapi(&gid)
        .await
        .map_err(|e| e.to_string())?;
    services
        .smapi
        .get_smapi_status(&gid)
        .map_err(|e| e.to_string())
}
