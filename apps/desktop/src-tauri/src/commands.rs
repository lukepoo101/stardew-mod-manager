use crate::events;
use crate::ipc::{self, IntoIpcResult, IpcResult};
use crate::state::AppState;
use manager_app::api::dto::*;
use manager_app::error::AppResult;
use manager_core::ids::*;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tauri::State;

// ---------------------------------------------------------------------------
// Bootstrap & Context
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> IpcResult<BootstrapDto> {
    state.services.bootstrap.get_bootstrap().into_ipc()
}

#[tauri::command]
pub fn set_onboarding_disposition<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    disposition: String,
) -> IpcResult<()> {
    let disp = match disposition.as_str() {
        "completed" => manager_core::profile::OnboardingDisposition::Completed,
        "skipped" => manager_core::profile::OnboardingDisposition::Skipped,
        _ => manager_core::profile::OnboardingDisposition::NotStarted,
    };
    events::after_state_change(&app, || {
        state
            .services
            .bootstrap
            .set_onboarding_disposition(disp)
            .into_ipc()
    })
}

// ---------------------------------------------------------------------------
// Games
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn discover_game_installations(
    state: State<'_, AppState>,
) -> IpcResult<Vec<GameInspectionDto>> {
    state.services.games.discover_games().into_ipc()
}

#[tauri::command]
pub fn validate_game_installation_path(
    state: State<'_, AppState>,
    path: String,
) -> IpcResult<GameInspectionDto> {
    state
        .services
        .games
        .inspect_path(Path::new(&path), None)
        .into_ipc()
}

#[tauri::command]
pub fn register_game_installation<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    path: String,
    storefront: Option<String>,
) -> IpcResult<GameInstallationSummaryDto> {
    let sf = match storefront.as_deref() {
        Some("steam") => manager_core::game::Storefront::Steam,
        Some("gog") => manager_core::game::Storefront::Gog,
        _ => manager_core::game::Storefront::Manual,
    };
    events::after_state_change(&app, || {
        state
            .services
            .games
            .accept_game(
                Path::new(&path),
                sf,
                manager_core::game::ManagementMode::Managed,
            )
            .into_ipc()
    })
}

#[tauri::command]
pub fn list_game_installations(
    state: State<'_, AppState>,
) -> IpcResult<Vec<GameInstallationSummaryDto>> {
    state.services.games.list_games().into_ipc()
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_profiles(
    state: State<'_, AppState>,
    game_id: Option<String>,
) -> IpcResult<Vec<ProfileSummaryDto>> {
    let gid = active_game_id(&state, game_id).into_ipc()?;
    state.services.profiles.list_profiles(&gid).into_ipc()
}

#[tauri::command]
pub fn list_archived_profiles(
    state: State<'_, AppState>,
    game_id: Option<String>,
) -> IpcResult<Vec<ProfileSummaryDto>> {
    let gid = active_game_id(&state, game_id).into_ipc()?;
    state
        .services
        .profiles
        .list_archived_profiles(&gid)
        .into_ipc()
}

#[tauri::command]
pub fn create_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    game_id: String,
    name: String,
    description: Option<String>,
) -> IpcResult<ProfileSummaryDto> {
    let gid = GameInstallationId::from_str(&game_id)
        .map_err(ipc::invalid_game_installation_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state
            .services
            .profiles
            .create_profile(&gid, &name, description.as_deref())
            .into_ipc()
    })
}

#[tauri::command]
pub fn activate_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    game_id: Option<String>,
) -> IpcResult<()> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    let gid = match game_id {
        Some(gid_str) => GameInstallationId::from_str(&gid_str)
            .map_err(ipc::invalid_game_installation_id)
            .into_ipc()?,
        None => {
            let profile = state
                .services
                .profiles
                .get_profile(&pid)
                .into_ipc()?
                .ok_or_else(ipc::profile_not_found)
                .into_ipc()?;
            GameInstallationId::from_str(&profile.game_installation_id)
                .map_err(ipc::invalid_game_installation_id)
                .into_ipc()?
        }
    };
    events::after_state_change(&app, || {
        state
            .services
            .profiles
            .switch_active_profile(&gid, &pid)
            .into_ipc()
    })
}

#[tauri::command]
pub fn archive_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<()> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.profiles.archive_profile(&pid).into_ipc()
    })
}

#[tauri::command]
pub fn restore_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<()> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.profiles.restore_profile(&pid).into_ipc()
    })
}

#[tauri::command]
pub fn get_active_profile_overview(state: State<'_, AppState>) -> IpcResult<ProfileOverviewDto> {
    let bootstrap = state.services.bootstrap.get_bootstrap().into_ipc()?;
    let pid_str = bootstrap
        .active_profile_id
        .ok_or_else(ipc::no_active_profile)
        .into_ipc()?;
    let pid = ProfileId::from_str(&pid_str)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    state.profile_queries.get_profile_overview(&pid).into_ipc()
}

// ---------------------------------------------------------------------------
// Mods & Queries
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_profile_mods(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> IpcResult<Vec<ModListItemDto>> {
    let pid_str = match profile_id {
        Some(id) => id,
        None => state
            .services
            .bootstrap
            .get_bootstrap()
            .into_ipc()?
            .active_profile_id
            .ok_or_else(ipc::no_active_profile)
            .into_ipc()?,
    };
    let pid = ProfileId::from_str(&pid_str)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    state.mods_queries.list_profile_mods(&pid).into_ipc()
}

#[tauri::command]
pub fn get_mod_details(
    state: State<'_, AppState>,
    profile_component_id: String,
) -> IpcResult<Option<ModDetailsDto>> {
    let cid = ProfileComponentId::from_str(&profile_component_id)
        .map_err(ipc::invalid_profile_component_id)
        .into_ipc()?;
    state.mods_queries.get_mod_details(&cid).into_ipc()
}

#[tauri::command]
pub fn inspect_package_for_install<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    archive_path: String,
    profile_id: Option<String>,
) -> IpcResult<OperationPreviewDto> {
    let pid_str = match profile_id {
        Some(id) => id,
        None => state
            .services
            .bootstrap
            .get_bootstrap()
            .into_ipc()?
            .active_profile_id
            .ok_or_else(ipc::no_active_profile)
            .into_ipc()?,
    };
    let pid = ProfileId::from_str(&pid_str)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    let resolved_path = resolve_mod_file_path(&archive_path).into_ipc()?;
    events::after_state_change(&app, || {
        state
            .services
            .mods
            .prepare_install(&pid, &resolved_path)
            .into_ipc()
    })
}

#[tauri::command]
pub fn prepare_remove<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_component_id: String,
) -> IpcResult<OperationPreviewDto> {
    let cid = ProfileComponentId::from_str(&profile_component_id)
        .map_err(ipc::invalid_profile_component_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.mods.prepare_removal(&cid).into_ipc()
    })
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn execute_operation<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    operation_id: String,
) -> IpcResult<OperationDto> {
    let op_id = OperationId::from_str(&operation_id)
        .map_err(ipc::invalid_operation_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state
            .services
            .operations
            .commit_operation(&op_id)
            .into_ipc()
    })
}

#[tauri::command]
pub fn get_operation_details(
    state: State<'_, AppState>,
    operation_id: String,
) -> IpcResult<Option<OperationDto>> {
    let op_id = OperationId::from_str(&operation_id)
        .map_err(ipc::invalid_operation_id)
        .into_ipc()?;
    state.services.operations.get_operation(&op_id).into_ipc()
}

#[tauri::command]
pub fn list_recent_operations(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    limit: Option<usize>,
) -> IpcResult<Vec<OperationDto>> {
    let pid = match profile_id {
        Some(s) if !s.is_empty() => Some(
            ProfileId::from_str(&s)
                .map_err(ipc::invalid_profile_id)
                .into_ipc()?,
        ),
        _ => None,
    };
    let mut list = state
        .services
        .operations
        .list_operations(pid.as_ref())
        .into_ipc()?;
    list.truncate(limit.unwrap_or(50));
    Ok(list)
}

#[tauri::command]
pub fn retry_recovery<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
) -> IpcResult<()> {
    events::after_state_change(&app, || {
        state.services.operations.retry_recovery().into_ipc()
    })
}

#[tauri::command]
pub fn cancel_active_operation<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    operation_id: String,
) -> IpcResult<()> {
    let id = OperationId::from_str(&operation_id)
        .map_err(ipc::invalid_operation_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.operations.cancel_operation(&id).into_ipc()
    })
}

// ---------------------------------------------------------------------------
// SMAPI
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_smapi_status(
    state: State<'_, AppState>,
    game_id: Option<String>,
) -> IpcResult<SmapiStatusDto> {
    let gid = active_game_id(&state, game_id).into_ipc()?;
    state.services.smapi.get_smapi_status(&gid).into_ipc()
}

// ---------------------------------------------------------------------------
// Launch & Session
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn launch_active_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    mode: Option<String>,
    profile_id: Option<String>,
) -> IpcResult<LaunchSessionDto> {
    let mode = match mode.as_deref() {
        None | Some("Modded" | "modded") => manager_core::launch::LaunchMode::Modded,
        Some("Vanilla" | "vanilla") => manager_core::launch::LaunchMode::Vanilla,
        Some("RuntimeTest" | "runtime_test") => manager_core::launch::LaunchMode::RuntimeTest,
        Some(value) => return Err(ipc::invalid_launch_mode(value).into()),
    };
    let pid_str = match profile_id {
        Some(id) => id,
        None => state
            .services
            .bootstrap
            .get_bootstrap()
            .into_ipc()?
            .active_profile_id
            .ok_or_else(ipc::no_active_profile)
            .into_ipc()?,
    };
    let pid = ProfileId::from_str(&pid_str)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.launch.launch_profile(&pid, mode).into_ipc()
    })
}

#[tauri::command]
pub fn get_active_launch_session(
    state: State<'_, AppState>,
) -> IpcResult<Option<LaunchSessionDto>> {
    let session = state.services.launch.get_latest_session(None).into_ipc()?;
    if let Some(session) = session {
        let id = LaunchSessionId::from_str(&session.id)
            .map_err(ipc::invalid_launch_session_id)
            .into_ipc()?;
        let Some(s) = state.services.launch.poll_session(&id).into_ipc()? else {
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
pub fn terminate_active_launch_session<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> IpcResult<()> {
    // Absence of a session is a request-level precondition; a repository
    // failure while looking one up is not, and must cross IPC as an error
    // rather than being flattened into "no active session".
    let session_id = match session_id {
        Some(id) => id,
        None => state
            .services
            .launch
            .get_latest_session(None)
            .into_ipc()?
            .map(|session| session.id)
            .ok_or_else(ipc::no_active_launch_session)
            .into_ipc()?,
    };
    let id = LaunchSessionId::from_str(&session_id)
        .map_err(ipc::invalid_launch_session_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.launch.terminate_game(Some(&id)).into_ipc()
    })
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_diagnostics_report(
    state: State<'_, AppState>,
    game_installation_id: Option<String>,
    session_id: Option<String>,
) -> IpcResult<DiagnosticsDto> {
    let _ = game_installation_id;
    let sid = match session_id {
        Some(s) if !s.is_empty() => Some(
            LaunchSessionId::from_str(&s)
                .map_err(ipc::invalid_launch_session_id)
                .into_ipc()?,
        ),
        _ => None,
    };
    state
        .services
        .diagnostics
        .get_diagnostics(sid.as_ref())
        .into_ipc()
}

// ---------------------------------------------------------------------------
// Native Dialogs
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn pick_folder_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> IpcResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Select Stardew Valley Game Directory")
        .pick_folder(move |folder| {
            let _ = tx.send(folder.map(|p| p.to_string()));
        });

    // Cancelling the picker resolves to None and stays a successful response.
    rx.await.map_err(ipc::native_dialog_failed).into_ipc()
}

#[tauri::command]
pub async fn pick_archive_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> IpcResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Select Stardew Valley Mod ZIP")
        .add_filter("ZIP Archives", &["zip"])
        .pick_file(move |file| {
            let _ = tx.send(file.map(|p| p.to_string()));
        });

    rx.await.map_err(ipc::native_dialog_failed).into_ipc()
}

fn resolve_mod_file_path(file_path: &str) -> AppResult<PathBuf> {
    // An unusable request is rejected before the environment is probed, so the
    // boundary code for a blank path does not depend on how the host is set up.
    if file_path.trim().is_empty() {
        return Err(ipc::mod_archive_path_required());
    }
    let home = std::env::var("HOME").map_err(ipc::home_directory_unavailable)?;
    resolve_mod_file_path_in_home(file_path, Path::new(&home))
}

fn resolve_mod_file_path_in_home(file_path: &str, home_path: &Path) -> AppResult<PathBuf> {
    let mut trimmed = file_path.trim();
    if trimmed.is_empty() {
        return Err(ipc::mod_archive_path_required());
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

    Err(ipc::mod_archive_not_found(
        "The mod archive could not be found",
        format!(
            "File '{}' does not exist. Looked in '{}' and '{}/Downloads/{}'",
            trimmed,
            expanded.display(),
            home,
            filename
        ),
    ))
}

fn active_game_id(state: &AppState, requested: Option<String>) -> AppResult<GameInstallationId> {
    let id = match requested {
        Some(id) => id,
        None => state
            .services
            .bootstrap
            .get_bootstrap()?
            .active_game_installation_id
            .ok_or_else(ipc::no_active_game)?,
    };
    GameInstallationId::from_str(&id).map_err(ipc::invalid_game_installation_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_app::error::AppErrorCategory;

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

    #[test]
    fn test_resolve_mod_file_path_reports_structured_errors() {
        let temporary_home = tempfile::tempdir().unwrap();

        let missing_path = resolve_mod_file_path_in_home("   ", temporary_home.path()).unwrap_err();
        assert_eq!(missing_path.code, ipc::MOD_ARCHIVE_PATH_REQUIRED);
        assert_eq!(missing_path.category, AppErrorCategory::Validation);
        assert_eq!(missing_path.summary, "No mod archive path was provided");

        let not_found =
            resolve_mod_file_path_in_home("definitely-missing.zip", temporary_home.path())
                .unwrap_err();
        assert_eq!(not_found.code, ipc::MOD_ARCHIVE_NOT_FOUND);
        assert_eq!(not_found.category, AppErrorCategory::Filesystem);
        assert!(not_found
            .technical_details
            .as_deref()
            .unwrap_or_default()
            .contains("definitely-missing.zip"));
    }
}
