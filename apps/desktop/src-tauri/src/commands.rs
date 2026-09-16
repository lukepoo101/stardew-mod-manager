use crate::state::AppState;
use manager_app::api::dto::*;
use manager_core::ids::*;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tauri::State;

// ---------------------------------------------------------------------------
// Bootstrap & Context
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Result<BootstrapDto, String> {
    let bootstrap = state
        .services
        .bootstrap
        .get_bootstrap()
        .map_err(|e| e.to_string())?;
    Ok(bootstrap)
}

#[tauri::command]
pub fn set_onboarding_disposition(
    state: State<'_, AppState>,
    disposition: String,
) -> Result<(), String> {
    let disp = match disposition.as_str() {
        "completed" => manager_core::profile::OnboardingDisposition::Completed,
        "skipped" => manager_core::profile::OnboardingDisposition::Skipped,
        _ => manager_core::profile::OnboardingDisposition::NotStarted,
    };
    state
        .services
        .bootstrap
        .set_onboarding_disposition(disp)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Games
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn discover_game_installations(
    state: State<'_, AppState>,
) -> Result<Vec<GameInspectionDto>, String> {
    state
        .services
        .games
        .discover_games()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn validate_game_installation_path(
    state: State<'_, AppState>,
    path: String,
) -> Result<GameInspectionDto, String> {
    state
        .services
        .games
        .inspect_path(Path::new(&path), None)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn register_game_installation(
    state: State<'_, AppState>,
    path: String,
    storefront: Option<String>,
) -> Result<GameInstallationSummaryDto, String> {
    let sf = match storefront.as_deref() {
        Some("steam") => manager_core::game::Storefront::Steam,
        Some("gog") => manager_core::game::Storefront::Gog,
        _ => manager_core::game::Storefront::Manual,
    };
    state
        .services
        .games
        .accept_game(
            Path::new(&path),
            sf,
            manager_core::game::ManagementMode::Managed,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_game_installations(
    state: State<'_, AppState>,
) -> Result<Vec<GameInstallationSummaryDto>, String> {
    state.services.games.list_games().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_profiles(
    state: State<'_, AppState>,
    game_id: Option<String>,
) -> Result<Vec<ProfileSummaryDto>, String> {
    let gid = active_game_id(&state, game_id)?;
    state
        .services
        .profiles
        .list_profiles(&gid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_archived_profiles(
    state: State<'_, AppState>,
    game_id: Option<String>,
) -> Result<Vec<ProfileSummaryDto>, String> {
    let gid = active_game_id(&state, game_id)?;
    state
        .services
        .profiles
        .list_archived_profiles(&gid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_profile(
    state: State<'_, AppState>,
    game_id: String,
    name: String,
    description: Option<String>,
) -> Result<ProfileSummaryDto, String> {
    let gid = GameInstallationId::from_str(&game_id).map_err(|e| e.to_string())?;
    state
        .services
        .profiles
        .create_profile(&gid, &name, description.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn activate_profile(
    state: State<'_, AppState>,
    profile_id: String,
    game_id: Option<String>,
) -> Result<(), String> {
    let pid = ProfileId::from_str(&profile_id).map_err(|e| e.to_string())?;
    let gid = if let Some(ref gid_str) = game_id {
        GameInstallationId::from_str(gid_str).map_err(|e| e.to_string())?
    } else {
        let prof = state
            .services
            .profiles
            .get_profile(&pid)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Profile not found".to_string())?;
        GameInstallationId::from_str(&prof.game_installation_id).map_err(|e| e.to_string())?
    };
    state
        .services
        .profiles
        .switch_active_profile(&gid, &pid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_profile(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    let pid = ProfileId::from_str(&profile_id).map_err(|e| e.to_string())?;
    state
        .services
        .profiles
        .archive_profile(&pid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn restore_profile(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    let pid = ProfileId::from_str(&profile_id).map_err(|e| e.to_string())?;
    state
        .services
        .profiles
        .restore_profile(&pid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_active_profile_overview(
    state: State<'_, AppState>,
) -> Result<ProfileOverviewDto, String> {
    let bootstrap = state
        .services
        .bootstrap
        .get_bootstrap()
        .map_err(|e| e.to_string())?;
    let pid_str = bootstrap
        .active_profile_id
        .ok_or_else(|| "No active profile".to_string())?;
    let pid = ProfileId::from_str(&pid_str).map_err(|e| e.to_string())?;
    state
        .profile_queries
        .get_profile_overview(&pid)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Mods & Queries
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_profile_mods(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<Vec<ModListItemDto>, String> {
    let pid_str = match profile_id {
        Some(id) => id,
        None => state
            .services
            .bootstrap
            .get_bootstrap()
            .map_err(|e| e.to_string())?
            .active_profile_id
            .ok_or_else(|| "No active profile".to_string())?,
    };
    let pid = ProfileId::from_str(&pid_str).map_err(|e| e.to_string())?;
    state
        .mods_queries
        .list_profile_mods(&pid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_mod_details(
    state: State<'_, AppState>,
    profile_component_id: String,
) -> Result<Option<ModDetailsDto>, String> {
    let cid = ProfileComponentId::from_str(&profile_component_id).map_err(|e| e.to_string())?;
    state
        .mods_queries
        .get_mod_details(&cid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn inspect_package_for_install(
    state: State<'_, AppState>,
    archive_path: String,
    profile_id: Option<String>,
) -> Result<OperationPreviewDto, String> {
    let pid_str = match profile_id {
        Some(id) => id,
        None => state
            .services
            .bootstrap
            .get_bootstrap()
            .map_err(|e| e.to_string())?
            .active_profile_id
            .ok_or_else(|| "No active profile".to_string())?,
    };
    let pid = ProfileId::from_str(&pid_str).map_err(|e| e.to_string())?;
    let resolved_path = resolve_mod_file_path(&archive_path)?;
    state
        .services
        .mods
        .prepare_install(&pid, &resolved_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn prepare_remove(
    state: State<'_, AppState>,
    profile_component_id: String,
) -> Result<OperationPreviewDto, String> {
    let cid = ProfileComponentId::from_str(&profile_component_id).map_err(|e| e.to_string())?;
    state
        .services
        .mods
        .prepare_removal(&cid)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn execute_operation(
    state: State<'_, AppState>,
    operation_id: String,
) -> Result<OperationDto, String> {
    let op_id = OperationId::from_str(&operation_id).map_err(|e| e.to_string())?;
    state
        .services
        .operations
        .commit_operation(&op_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_operation_details(
    state: State<'_, AppState>,
    operation_id: String,
) -> Result<Option<OperationDto>, String> {
    let op_id = OperationId::from_str(&operation_id).map_err(|e| e.to_string())?;
    state
        .services
        .operations
        .get_operation(&op_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_recent_operations(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<OperationDto>, String> {
    let pid = match profile_id {
        Some(s) if !s.is_empty() => Some(ProfileId::from_str(&s).map_err(|e| e.to_string())?),
        _ => None,
    };
    let mut list = state
        .services
        .operations
        .list_operations(pid.as_ref())
        .map_err(|e| e.to_string())?;
    list.truncate(limit.unwrap_or(50));
    Ok(list)
}

#[tauri::command]
pub fn retry_recovery(state: State<'_, AppState>) -> Result<(), String> {
    state
        .services
        .operations
        .retry_recovery()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn cancel_active_operation(
    state: State<'_, AppState>,
    operation_id: String,
) -> Result<(), String> {
    let id = OperationId::from_str(&operation_id).map_err(|e| e.to_string())?;
    state
        .services
        .operations
        .cancel_operation(&id)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// SMAPI
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_smapi_status(
    state: State<'_, AppState>,
    game_id: Option<String>,
) -> Result<SmapiStatusDto, String> {
    let gid = active_game_id(&state, game_id)?;
    state
        .services
        .smapi
        .get_smapi_status(&gid)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Launch & Session
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn launch_active_profile(
    state: State<'_, AppState>,
    mode: Option<String>,
    profile_id: Option<String>,
) -> Result<LaunchSessionDto, String> {
    let mode = match mode.as_deref() {
        None | Some("Modded" | "modded") => manager_core::launch::LaunchMode::Modded,
        Some("Vanilla" | "vanilla") => manager_core::launch::LaunchMode::Vanilla,
        Some("RuntimeTest" | "runtime_test") => manager_core::launch::LaunchMode::RuntimeTest,
        Some(value) => return Err(format!("Unknown launch mode: {value}")),
    };
    let pid_str = match profile_id {
        Some(id) => id,
        None => state
            .services
            .bootstrap
            .get_bootstrap()
            .map_err(|e| e.to_string())?
            .active_profile_id
            .ok_or_else(|| "No active profile".to_string())?,
    };
    let pid = ProfileId::from_str(&pid_str).map_err(|e| e.to_string())?;
    state
        .services
        .launch
        .launch_profile(&pid, mode)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_active_launch_session(
    state: State<'_, AppState>,
) -> Result<Option<LaunchSessionDto>, String> {
    let session = state
        .services
        .launch
        .get_latest_session(None)
        .map_err(|e| e.to_string())?;
    if let Some(session) = session {
        let id = LaunchSessionId::from_str(&session.id).map_err(|e| e.to_string())?;
        let Some(s) = state
            .services
            .launch
            .poll_session(&id)
            .map_err(|e| e.to_string())?
        else {
            return Ok(None);
        };
        if s.ended_at.is_none()
            && (s.state == "starting"
                || s.state == "running_unverified"
                || s.state == "mod_load_confirmed"
                || s.state == "verification_unavailable")
        {
            return Ok(Some(s));
        }
    }
    Ok(None)
}

#[tauri::command]
pub fn terminate_active_launch_session(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<(), String> {
    let session_id = session_id
        .or_else(|| {
            state
                .services
                .launch
                .get_latest_session(None)
                .ok()
                .flatten()
                .map(|s| s.id)
        })
        .ok_or_else(|| "No active launch session".to_string())?;
    let id = LaunchSessionId::from_str(&session_id).map_err(|e| e.to_string())?;
    state
        .services
        .launch
        .terminate_game(Some(&id))
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_diagnostics_report(
    state: State<'_, AppState>,
    game_installation_id: Option<String>,
    session_id: Option<String>,
) -> Result<DiagnosticsDto, String> {
    let _ = game_installation_id;
    let sid = match session_id {
        Some(s) if !s.is_empty() => Some(LaunchSessionId::from_str(&s).map_err(|e| e.to_string())?),
        _ => None,
    };
    state
        .services
        .diagnostics
        .get_diagnostics(sid.as_ref())
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Native Dialogs
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn pick_folder_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Select Stardew Valley Game Directory")
        .pick_folder(move |folder| {
            let _ = tx.send(folder.map(|p| p.to_string()));
        });

    rx.await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pick_archive_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Select Stardew Valley Mod ZIP")
        .add_filter("ZIP Archives", &["zip"])
        .pick_file(move |file| {
            let _ = tx.send(file.map(|p| p.to_string()));
        });

    rx.await.map_err(|e| e.to_string())
}

fn resolve_mod_file_path(file_path: &str) -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set")?;
    resolve_mod_file_path_in_home(file_path, Path::new(&home))
}

fn resolve_mod_file_path_in_home(file_path: &str, home_path: &Path) -> Result<PathBuf, String> {
    let mut trimmed = file_path.trim();
    if trimmed.is_empty() {
        return Err("No file path provided".to_string());
    }

    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        trimmed = trimmed[1..trimmed.len() - 1].trim();
    }

    let mut unescaped = if let Some(stripped) = trimmed.strip_prefix("file://") {
        stripped.to_string()
    } else {
        trimmed.to_string()
    };

    if unescaped.contains('%') {
        let mut decoded = String::with_capacity(unescaped.len());
        let mut chars = unescaped.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%' {
                let h1 = chars.next();
                let h2 = chars.next();
                if let (Some(h1), Some(h2)) = (h1, h2) {
                    if let Ok(byte) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                        decoded.push(byte as char);
                        continue;
                    } else {
                        decoded.push('%');
                        decoded.push(h1);
                        decoded.push(h2);
                        continue;
                    }
                } else {
                    decoded.push('%');
                    if let Some(h1) = h1 {
                        decoded.push(h1);
                    }
                    continue;
                }
            } else {
                decoded.push(c);
            }
        }
        unescaped = decoded;
    }

    let candidate = unescaped.trim();
    let home = home_path.to_string_lossy().into_owned();
    let expanded = if let Some(stripped) = candidate.strip_prefix("~/") {
        PathBuf::from(&home).join(stripped)
    } else {
        PathBuf::from(candidate)
    };

    if expanded.exists() {
        return Ok(expanded);
    }

    let in_downloads = Path::new(&home).join("Downloads").join(candidate);
    if in_downloads.exists() {
        return Ok(in_downloads);
    }

    let filename = Path::new(candidate)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| candidate.to_string());

    let in_downloads_by_name = Path::new(&home).join("Downloads").join(&filename);
    if in_downloads_by_name.exists() {
        return Ok(in_downloads_by_name);
    }

    let in_desktop = Path::new(&home).join("Desktop").join(&filename);
    if in_desktop.exists() {
        return Ok(in_desktop);
    }

    Err(format!(
        "File '{}' does not exist. Looked in '{}' and '{}/Downloads/{}'",
        trimmed,
        expanded.display(),
        home,
        filename
    ))
}

fn active_game_id(
    state: &AppState,
    requested: Option<String>,
) -> Result<GameInstallationId, String> {
    let id = match requested {
        Some(id) => id,
        None => state
            .services
            .bootstrap
            .get_bootstrap()
            .map_err(|e| e.to_string())?
            .active_game_installation_id
            .ok_or("No active game")?,
    };
    GameInstallationId::from_str(&id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_mod_file_path_various_formats() {
        let temporary_home = tempfile::tempdir().unwrap();
        let home = temporary_home.path().to_string_lossy().into_owned();
        let downloads_dir = Path::new(&home).join("Downloads");
        if !downloads_dir.exists() {
            let _ = std::fs::create_dir_all(&downloads_dir);
        }
        let test_file = downloads_dir.join("test_mod_sample.zip");
        std::fs::write(&test_file, b"PK00").unwrap();

        let resolved =
            resolve_mod_file_path_in_home(test_file.to_str().unwrap(), temporary_home.path())
                .unwrap();
        assert_eq!(resolved, test_file);

        let resolved2 =
            resolve_mod_file_path_in_home("test_mod_sample.zip", temporary_home.path()).unwrap();
        assert_eq!(resolved2, test_file);

        let resolved3 =
            resolve_mod_file_path_in_home("~/Downloads/test_mod_sample.zip", temporary_home.path())
                .unwrap();
        assert_eq!(resolved3, test_file);

        let uri = format!("file://{}", test_file.to_str().unwrap());
        let resolved4 = resolve_mod_file_path_in_home(&uri, temporary_home.path()).unwrap();
        assert_eq!(resolved4, test_file);

        let quoted = format!("\"{}\"", test_file.to_str().unwrap());
        let resolved5 = resolve_mod_file_path_in_home(&quoted, temporary_home.path()).unwrap();
        assert_eq!(resolved5, test_file);

        let _ = std::fs::remove_file(test_file);
    }
}
