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

fn active_profile_id(state: &State<'_, AppState>) -> IpcResult<ProfileId> {
    let bootstrap = state.services.bootstrap.get_bootstrap().into_ipc()?;
    let pid_str = bootstrap
        .active_profile_id
        .ok_or_else(ipc::no_active_profile)
        .into_ipc()?;
    ProfileId::from_str(&pid_str)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()
}

#[tauri::command]
pub fn get_troubleshoot_status(
    state: State<'_, AppState>,
) -> IpcResult<manager_app::api::dto::TroubleshootDto> {
    let pid = active_profile_id(&state)?;
    state.services.troubleshoot.status(&pid).into_ipc()
}

#[tauri::command]
pub fn start_troubleshoot<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
) -> IpcResult<manager_app::api::dto::TroubleshootDto> {
    let pid = active_profile_id(&state)?;
    events::after_state_change(&app, || state.services.troubleshoot.start(&pid).into_ipc())
}

#[tauri::command]
pub fn answer_troubleshoot<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    problem_present: bool,
) -> IpcResult<manager_app::api::dto::TroubleshootDto> {
    let pid = active_profile_id(&state)?;
    events::after_state_change(&app, || {
        state
            .services
            .troubleshoot
            .answer(&pid, problem_present)
            .into_ipc()
    })
}

#[tauri::command]
pub fn restore_troubleshoot<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
) -> IpcResult<manager_app::api::dto::TroubleshootDto> {
    let pid = active_profile_id(&state)?;
    events::after_state_change(&app, || {
        state.services.troubleshoot.restore(&pid).into_ipc()
    })
}

#[tauri::command]
pub fn list_dismissed_findings(
    state: State<'_, AppState>,
) -> IpcResult<Vec<manager_app::api::dto::DismissedFindingDto>> {
    manager_app::services::FindingDismissals::new(state.repo.clone())
        .list()
        .into_ipc()
}

#[tauri::command]
pub fn dismiss_finding<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    fingerprint: String,
    signature: String,
    severity: String,
) -> IpcResult<()> {
    events::after_state_change(&app, || {
        manager_app::services::FindingDismissals::new(state.repo.clone())
            .dismiss(&fingerprint, &signature, &severity)
            .into_ipc()
    })
}

#[tauri::command]
pub fn restore_finding<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    fingerprint: String,
) -> IpcResult<()> {
    events::after_state_change(&app, || {
        manager_app::services::FindingDismissals::new(state.repo.clone())
            .restore(&fingerprint)
            .into_ipc()
    })
}

#[tauri::command]
pub fn get_toggle_impact(
    state: State<'_, AppState>,
    profile_component_id: String,
    enable: bool,
) -> IpcResult<manager_app::api::dto::ToggleImpactDto> {
    let cid = ProfileComponentId::from_str(&profile_component_id)
        .map_err(ipc::invalid_profile_component_id)
        .into_ipc()?;
    state.services.toggle.impact(&cid, enable).into_ipc()
}

#[tauri::command]
pub fn set_mod_enabled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_component_id: String,
    enabled: bool,
) -> IpcResult<()> {
    let cid = ProfileComponentId::from_str(&profile_component_id)
        .map_err(ipc::invalid_profile_component_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.toggle.set_enabled(&cid, enabled).into_ipc()
    })
}

#[tauri::command]
pub fn export_profile_bundle(
    state: State<'_, AppState>,
    destination_dir: String,
) -> IpcResult<manager_app::api::dto::BundleExportDto> {
    let pid = ProfileId::from_str(
        &state
            .services
            .bootstrap
            .get_bootstrap()
            .into_ipc()?
            .active_profile_id
            .ok_or_else(ipc::no_active_profile)
            .into_ipc()?,
    )
    .map_err(ipc::invalid_profile_id)
    .into_ipc()?;
    state
        .services
        .bundle
        .export_bundle(&pid, std::path::Path::new(&destination_dir))
        .into_ipc()
}

#[tauri::command]
pub fn inspect_profile_bundle(
    state: State<'_, AppState>,
    bundle_path: String,
) -> IpcResult<manager_app::api::dto::BundlePreviewDto> {
    let resolved = resolve_mod_file_path(&bundle_path).into_ipc()?;
    state.services.bundle.inspect_bundle(&resolved).into_ipc()
}

/// Importing installs many mods, so it runs off the UI thread.
#[tauri::command]
pub async fn import_profile_bundle<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    bundle_path: String,
    game_id: String,
    profile_name: String,
) -> IpcResult<manager_app::api::dto::BundleImportDto> {
    let resolved = resolve_mod_file_path(&bundle_path).into_ipc()?;
    let gid = GameInstallationId::from_str(&game_id)
        .map_err(ipc::invalid_game_installation_id)
        .into_ipc()?;
    let bundle = state.services.bundle.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        bundle.import_bundle(&resolved, &gid, &profile_name)
    })
    .await
    .map_err(|e| {
        manager_app::error::AppError::internal("The import stopped unexpectedly", e.to_string())
    })
    .into_ipc()?;
    // Announced after the fact, on success and failure, because a partial
    // import still changed state.
    events::emit_backend_state_changed(&app);
    outcome.into_ipc()
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
pub fn get_launch_preflight(
    state: State<'_, AppState>,
    mode: Option<String>,
    profile_id: Option<String>,
) -> IpcResult<manager_app::api::dto::PreflightDto> {
    let mode = match mode.as_deref() {
        None | Some("Modded" | "modded") => manager_core::launch::LaunchMode::Modded,
        Some("Vanilla" | "vanilla") => manager_core::launch::LaunchMode::Vanilla,
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
    let check = state
        .services
        .launch
        .get_launch_preflight(&pid, mode)
        .into_ipc()?;
    Ok(manager_app::api::dto::PreflightDto {
        can_launch: check.can_launch,
        blockers: check.blockers,
        warnings: check.warnings,
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
    // An absolute path needs no home directory, and on Windows there may be no
    // HOME at all: the variable is a POSIX convention, and Windows sets
    // USERPROFILE instead. Requiring HOME made every manual archive path fail
    // on a default Windows profile.
    let path = Path::new(file_path.trim());
    if path.is_absolute() {
        return resolve_mod_file_path_in_home(file_path, None);
    }
    resolve_mod_file_path_in_home(file_path, user_home_directory().as_deref())
}

/// The current user's home directory, on any platform.
fn user_home_directory() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"].iter().find_map(|name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|value| !value.as_os_str().is_empty())
    })
}

fn resolve_mod_file_path_in_home(file_path: &str, home_path: Option<&Path>) -> AppResult<PathBuf> {
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
    let expanded = if let Some(stripped) = candidate.strip_prefix("~/") {
        let home = home_path.ok_or_else(|| ipc::home_directory_unavailable("HOME is not set"))?;
        home.join(stripped)
    } else {
        PathBuf::from(candidate)
    };

    if expanded.exists() {
        return Ok(expanded);
    }

    // The remaining rules are conveniences that need a home directory to search
    // in. Without one, the path is still resolved if it exists, and otherwise
    // the error below explains what was tried.
    let filename = Path::new(candidate)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| candidate.to_string());

    let Some(home) = home_path else {
        // Without a home directory the path itself is still resolved above; the
        // convenience searches are simply unavailable, and the error says that
        // instead of claiming the file is missing from a place never searched.
        return Err(ipc::mod_archive_not_found(
            "The mod archive could not be found",
            format!(
                "File '{}' does not exist, and there is no home directory to search for it in",
                trimmed
            ),
        ));
    };

    let in_downloads = home.join("Downloads").join(candidate);
    if in_downloads.exists() {
        return Ok(in_downloads);
    }

    let in_downloads_by_name = home.join("Downloads").join(&filename);
    if in_downloads_by_name.exists() {
        return Ok(in_downloads_by_name);
    }

    let in_desktop = home.join("Desktop").join(&filename);
    if in_desktop.exists() {
        return Ok(in_desktop);
    }

    Err(ipc::mod_archive_not_found(
        "The mod archive could not be found",
        format!(
            "File '{}' does not exist. Looked in '{}' and '{}/Downloads/{}'",
            trimmed,
            expanded.display(),
            home.display(),
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
            resolve_mod_file_path_in_home(test_file.to_str().unwrap(), Some(temporary_home.path()))
                .unwrap();
        assert_eq!(resolved, test_file);

        let resolved2 =
            resolve_mod_file_path_in_home("test_mod_sample.zip", Some(temporary_home.path()))
                .unwrap();
        assert_eq!(resolved2, test_file);

        let resolved3 = resolve_mod_file_path_in_home(
            "~/Downloads/test_mod_sample.zip",
            Some(temporary_home.path()),
        )
        .unwrap();
        assert_eq!(resolved3, test_file);

        let uri = format!("file://{}", test_file.to_str().unwrap());
        let resolved4 = resolve_mod_file_path_in_home(&uri, Some(temporary_home.path())).unwrap();
        assert_eq!(resolved4, test_file);

        let quoted = format!("\"{}\"", test_file.to_str().unwrap());
        let resolved5 =
            resolve_mod_file_path_in_home(&quoted, Some(temporary_home.path())).unwrap();
        assert_eq!(resolved5, test_file);

        let _ = std::fs::remove_file(test_file);
    }

    #[test]
    fn test_resolve_mod_file_path_reports_structured_errors() {
        let temporary_home = tempfile::tempdir().unwrap();

        let missing_path =
            resolve_mod_file_path_in_home("   ", Some(temporary_home.path())).unwrap_err();
        assert_eq!(missing_path.code, ipc::MOD_ARCHIVE_PATH_REQUIRED);
        assert_eq!(missing_path.category, AppErrorCategory::Validation);
        assert_eq!(missing_path.summary, "No mod archive path was provided");

        let not_found =
            resolve_mod_file_path_in_home("definitely-missing.zip", Some(temporary_home.path()))
                .unwrap_err();
        assert_eq!(not_found.code, ipc::MOD_ARCHIVE_NOT_FOUND);
        assert_eq!(not_found.category, AppErrorCategory::Filesystem);
        assert!(not_found
            .technical_details
            .as_deref()
            .unwrap_or_default()
            .contains("definitely-missing.zip"));
    }

    /// Windows does not set HOME: it is a POSIX convention and Windows sets
    /// USERPROFILE instead. Requiring HOME made every manually entered archive
    /// path fail on a default Windows profile, which the packaged smoke test
    /// caught.
    #[test]
    fn an_absolute_archive_path_resolves_without_a_home_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("Mod.zip");
        std::fs::write(&archive, b"PK\x03\x04").unwrap();

        let resolved = resolve_mod_file_path_in_home(archive.to_str().unwrap(), None)
            .expect("an absolute path needs no home directory");
        assert_eq!(resolved, archive);
    }

    #[test]
    fn a_tilde_path_reports_the_missing_home_rather_than_guessing() {
        let error = resolve_mod_file_path_in_home("~/Downloads/Mod.zip", None).unwrap_err();
        assert_eq!(error.code, ipc::HOME_DIRECTORY_UNAVAILABLE);
    }

    #[test]
    fn a_relative_path_without_a_home_reports_not_found_not_a_home_failure() {
        // The file genuinely does not exist, so the answer is "not found"; the
        // absent home directory only removes the convenience searches.
        let error = resolve_mod_file_path_in_home("definitely-missing.zip", None).unwrap_err();
        assert_eq!(error.code, ipc::MOD_ARCHIVE_NOT_FOUND);
        assert!(error
            .technical_details
            .as_deref()
            .unwrap_or_default()
            .contains("no home directory"));
    }
}

// ---------------------------------------------------------------------------
// Storage cleanup
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_cleanup_preview(
    state: State<'_, AppState>,
) -> IpcResult<manager_app::api::dto::CleanupPreviewDto> {
    state.services.storage.preview().into_ipc()
}

#[tauri::command]
pub fn run_cleanup<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    item_ids: Vec<String>,
) -> IpcResult<manager_app::api::dto::CleanupResultDto> {
    events::after_state_change(&app, || state.services.storage.run(&item_ids).into_ipc())
}

// Mod annotations and file locations

#[tauri::command]
pub fn list_mod_annotations(state: State<'_, AppState>) -> IpcResult<Vec<ModAnnotationDto>> {
    manager_app::services::ModAnnotations::new(state.repo.clone())
        .list()
        .into_ipc()
}

#[tauri::command]
pub fn set_mod_annotation<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    unique_id: String,
    favourite: bool,
    tags: Vec<String>,
    note: String,
) -> IpcResult<ModAnnotationDto> {
    events::after_state_change(&app, || {
        manager_app::services::ModAnnotations::new(state.repo.clone())
            .set(&unique_id, favourite, &tags, &note)
            .into_ipc()
    })
}

type ModRecord = (
    manager_core::deployment::ProfileComponent,
    manager_core::deployment::ProfileDeployment,
);

/// A profile component and the deployment that holds its files.
fn mod_record(state: &State<'_, AppState>, profile_component_id: &str) -> AppResult<ModRecord> {
    use manager_app::error::AppError;
    use manager_app::ports::repositories::DeploymentRepository;
    let cid = ProfileComponentId::from_str(profile_component_id)
        .map_err(|_| AppError::validation("COMPONENT_INVALID", "That mod id is not valid"))?;
    let component = state.repo.get_profile_component(&cid)?.ok_or_else(|| {
        AppError::validation("COMPONENT_NOT_FOUND", "That mod is not in any profile")
    })?;
    let deployment = state
        .repo
        .get_deployment(&component.deployment_id)?
        .ok_or_else(|| {
            AppError::validation("DEPLOYMENT_NOT_FOUND", "The mod's files are not recorded")
        })?;
    Ok((component, deployment))
}

/// Where a mod's folder is, resolved from the manager's records.
fn mod_files_path(state: &State<'_, AppState>, profile_component_id: &str) -> AppResult<PathBuf> {
    use manager_app::error::AppError;
    let (component, deployment) = mod_record(state, profile_component_id)?;
    let relative = Path::new(&deployment.root_relative_path);
    if relative
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(AppError::validation(
            "DEPLOYMENT_PATH_INVALID",
            "The recorded folder for this mod is not a plain relative path",
        ));
    }
    let live = state
        .paths
        .profile_mods_dir(&component.profile_id)
        .join(relative);
    let disabled = state
        .paths
        .profile_disabled_dir(&component.profile_id)
        .join(relative);
    // Prefer where the recorded state says it is, but find it either way.
    let (first, second) = if component.enabled {
        (live, disabled)
    } else {
        (disabled, live)
    };
    [first, second]
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            AppError::validation(
                "MOD_FILES_MISSING",
                "The mod's folder is not where the manager put it. Diagnostics may explain why.",
            )
        })
}

/// The retained archive a mod was installed from.
fn mod_package_path(state: &State<'_, AppState>, profile_component_id: &str) -> AppResult<PathBuf> {
    let (_, deployment) = mod_record(state, profile_component_id)?;
    let package = state.paths.package_path(deployment.artifact_hash.as_str());
    if package.exists() {
        Ok(package)
    } else {
        Err(manager_app::error::AppError::validation(
            "PACKAGE_NOT_RETAINED",
            "The archive this mod was installed from is no longer kept. It may have been installed before archives were retained, or removed by storage cleanup.",
        ))
    }
}

fn reveal(path: AppResult<PathBuf>) -> IpcResult<()> {
    let path = path.into_ipc()?;
    manager_infra::reveal::reveal_in_file_manager(&path)
        .map_err(|e| {
            manager_app::error::AppError::filesystem(
                "Could not open the file manager",
                e.to_string(),
            )
        })
        .into_ipc()
}

#[tauri::command]
pub fn reveal_mod_files(state: State<'_, AppState>, profile_component_id: String) -> IpcResult<()> {
    reveal(mod_files_path(&state, &profile_component_id))
}

#[tauri::command]
pub fn reveal_mod_package(
    state: State<'_, AppState>,
    profile_component_id: String,
) -> IpcResult<()> {
    reveal(mod_package_path(&state, &profile_component_id))
}

#[tauri::command]
pub fn update_profile_details<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    name: String,
    description: Option<String>,
) -> IpcResult<ProfileSummaryDto> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state
            .services
            .profiles
            .update_profile_details(&pid, &name, description.as_deref())
            .into_ipc()
    })
}

fn component_ids(ids: &[String]) -> AppResult<Vec<ProfileComponentId>> {
    ids.iter()
        .map(|id| {
            ProfileComponentId::from_str(id).map_err(|_| {
                manager_app::error::AppError::validation(
                    "COMPONENT_INVALID",
                    "One of the selected mods has an invalid id",
                )
            })
        })
        .collect()
}

#[tauri::command]
pub fn get_bulk_toggle_impact(
    state: State<'_, AppState>,
    profile_component_ids: Vec<String>,
    enable: bool,
) -> IpcResult<ToggleImpactDto> {
    let ids = component_ids(&profile_component_ids).into_ipc()?;
    state.services.toggle.impact_many(&ids, enable).into_ipc()
}

#[tauri::command]
pub fn set_mods_enabled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_component_ids: Vec<String>,
    enabled: bool,
) -> IpcResult<BulkToggleResultDto> {
    let ids = component_ids(&profile_component_ids).into_ipc()?;
    events::after_state_change(&app, || {
        state
            .services
            .toggle
            .set_many_enabled(&ids, enabled)
            .into_ipc()
    })
}
