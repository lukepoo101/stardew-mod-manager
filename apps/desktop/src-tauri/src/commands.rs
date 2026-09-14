use crate::state::AppState;
use manager_app::api::dto::*;
use manager_core::domain::*;
use manager_core::ids::*;
use manager_core::install::ArchiveInspectionResult;
use manager_core::ports::StateRepository;
use manager_core::use_cases::AppSnapshot;
use manager_infra::archive::SafeZipExtractor;
use manager_infra::discovery::SteamGameDiscovery;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tauri::State;

// ---------------------------------------------------------------------------
// Bootstrap & Context
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Result<BootstrapDto, String> {
    let mut bootstrap = state
        .services
        .bootstrap
        .get_bootstrap()
        .map_err(|e| e.to_string())?;
    if let Some(error) = state
        .recovery_error
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
    {
        bootstrap.recovery_summary = Some(error);
    }
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
pub fn inspect_game_path(
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
pub fn accept_game(
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
pub fn list_games(state: State<'_, AppState>) -> Result<Vec<GameInstallationSummaryDto>, String> {
    state.services.games.list_games().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_active_game(state: State<'_, AppState>, game_id: String) -> Result<(), String> {
    let gid = GameInstallationId::from_str(&game_id).map_err(|e| e.to_string())?;
    state
        .services
        .games
        .set_active_game(&gid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn discover_games(state: State<'_, AppState>) -> Result<Vec<GameInstallation>, String> {
    let mut discovered = SteamGameDiscovery::discover_installations();

    if let Ok(existing_games) = state.use_cases.repo.list_games() {
        for candidate in &mut discovered {
            if let Some(existing) = existing_games
                .iter()
                .find(|g| g.canonical_root == candidate.canonical_root)
            {
                let managed_val = manager_core::game::validate_managed_game(
                    &candidate.canonical_root,
                    candidate.platform_kind,
                );
                if managed_val.is_valid {
                    candidate.id = existing.id.clone();
                    candidate.is_managed = true;
                    candidate.validation_error = None;
                    candidate.detected_version = managed_val.detected_version;
                }
            }
        }

        for existing in existing_games {
            if !discovered
                .iter()
                .any(|c| c.canonical_root == existing.canonical_root)
            {
                let managed_val = manager_core::game::validate_managed_game(
                    &existing.canonical_root,
                    existing.platform_kind,
                );
                let mut managed_game = existing.clone();
                managed_game.is_managed = true;
                if managed_val.is_valid {
                    managed_game.validation_error = None;
                    managed_game.detected_version = managed_val.detected_version;
                }
                discovered.push(managed_game);
            }
        }
    }

    Ok(discovered)
}

#[tauri::command]
pub fn choose_game(
    state: State<'_, AppState>,
    folder_path: String,
) -> Result<GameInstallation, String> {
    let path = PathBuf::from(folder_path);
    let mut game = state
        .use_cases
        .inspect_game(&path, StoreKind::ManualFolder)?;
    if let Ok(existing_games) = state.use_cases.repo.list_games() {
        if let Some(existing) = existing_games
            .iter()
            .find(|g| g.canonical_root == game.canonical_root)
        {
            let managed_val =
                manager_core::game::validate_managed_game(&game.canonical_root, game.platform_kind);
            if managed_val.is_valid {
                game.id = existing.id.clone();
                game.is_managed = true;
                game.validation_error = None;
                game.detected_version = managed_val.detected_version;
            }
        }
    }
    Ok(game)
}

#[tauri::command]
pub fn select_game(
    state: State<'_, AppState>,
    candidate_path: Option<String>,
    candidate_id: Option<String>,
    platform_kind: Option<String>,
) -> Result<AppSnapshot, String> {
    let path_to_inspect = if let Some(ref p) = candidate_path {
        Some(PathBuf::from(p))
    } else if let Some(ref id) = candidate_id {
        if let Ok(Some(existing)) = state.use_cases.repo.get_game(id) {
            return state.use_cases.get_app_snapshot(Some(&existing.id));
        }
        let candidate_as_path = PathBuf::from(id);
        if candidate_as_path.exists() {
            Some(candidate_as_path)
        } else {
            SteamGameDiscovery::discover_installations()
                .into_iter()
                .find(|g| g.id == *id)
                .map(|g| g.canonical_root)
        }
    } else {
        None
    };

    if let Some(path) = path_to_inspect {
        let kind = match platform_kind.as_deref() {
            Some("steam_native") => StoreKind::SteamNative,
            _ => StoreKind::ManualFolder,
        };
        let mut game = state.use_cases.inspect_game(&path, kind)?;
        if let Ok(existing_games) = state.use_cases.repo.list_games() {
            if let Some(existing) = existing_games
                .iter()
                .find(|g| g.canonical_root == game.canonical_root)
            {
                let managed_val = manager_core::game::validate_managed_game(
                    &game.canonical_root,
                    game.platform_kind,
                );
                if managed_val.is_valid {
                    game.id = existing.id.clone();
                    game.is_managed = true;
                    game.validation_error = None;
                    game.detected_version = managed_val.detected_version;
                }
            }
        }
        let game = state.use_cases.accept_game(&game)?;
        state.use_cases.get_app_snapshot(Some(&game.id))
    } else {
        Err("No valid game candidate path or ID provided".to_string())
    }
}

#[tauri::command]
pub fn get_app_snapshot(
    state: State<'_, AppState>,
    game_id: Option<String>,
) -> Result<AppSnapshot, String> {
    let mut snapshot = state.use_cases.get_app_snapshot(game_id.as_deref())?;
    snapshot.recovery_error = state
        .recovery_error
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(snapshot)
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
pub fn get_profile(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<Option<ProfileSummaryDto>, String> {
    let pid = ProfileId::from_str(&profile_id).map_err(|e| e.to_string())?;
    state
        .services
        .profiles
        .get_profile(&pid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_active_profile(
    state: State<'_, AppState>,
    game_id: String,
) -> Result<Option<ProfileSummaryDto>, String> {
    let gid = GameInstallationId::from_str(&game_id).map_err(|e| e.to_string())?;
    state
        .services
        .profiles
        .get_active_profile(&gid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_profile_overview(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<ProfileOverviewDto, String> {
    let pid = ProfileId::from_str(&profile_id).map_err(|e| e.to_string())?;
    state
        .profile_queries
        .get_profile_overview(&pid)
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
pub fn select_profile(
    state: State<'_, AppState>,
    game_id: String,
    profile_id: String,
) -> Result<(), String> {
    let gid = GameInstallationId::from_str(&game_id).map_err(|e| e.to_string())?;
    let pid = ProfileId::from_str(&profile_id).map_err(|e| e.to_string())?;
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

// ---------------------------------------------------------------------------
// Mods & Queries
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_mods(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<Vec<ModListItemDto>, String> {
    let pid = ProfileId::from_str(&profile_id).map_err(|e| e.to_string())?;
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
pub fn toggle_mod(
    state: State<'_, AppState>,
    profile_component_id: String,
    enabled: bool,
) -> Result<(), String> {
    let cid = ProfileComponentId::from_str(&profile_component_id).map_err(|e| e.to_string())?;
    state
        .services
        .mods
        .toggle_mod(&cid, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn prepare_install(
    state: State<'_, AppState>,
    profile_id: String,
    source_path: String,
) -> Result<OperationPreviewDto, String> {
    let pid = ProfileId::from_str(&profile_id).map_err(|e| e.to_string())?;
    let resolved_path = resolve_mod_file_path(&source_path)?;
    state
        .services
        .mods
        .prepare_install(&pid, &resolved_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn prepare_remove(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    profile_component_id: String,
) -> Result<OperationPreviewDto, String> {
    let _ = profile_id;
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
pub fn commit_operation(
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
pub fn get_operation(
    state: State<'_, AppState>,
    id: Option<String>,
    operation_id: Option<String>,
) -> Result<Option<OperationDto>, String> {
    let target = operation_id.or(id).ok_or("No operation ID provided")?;
    let op_id = OperationId::from_str(&target).map_err(|e| e.to_string())?;
    state
        .services
        .operations
        .get_operation(&op_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_operations(
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
    let result = state.use_cases.recover_operations_with_resolver(|id| {
        (
            state.paths.mods_dir(id),
            state.paths.staging_dir(id, "inspect"),
            state.paths.recovery_dir(id, "remove"),
        )
    });
    *state.recovery_error.lock().map_err(|e| e.to_string())? = result.as_ref().err().cloned();
    result.map(|_| ())
}

#[tauri::command]
pub fn cancel_operation(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = (state, id);
    Err("Started operations cannot be cancelled; use recovery to reconcile them".into())
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

#[tauri::command]
pub fn prepare_smapi() -> Result<manager_core::smapi::SmapiReleaseInfo, String> {
    Ok(manager_core::smapi::get_pinned_smapi_release())
}

#[tauri::command]
pub fn install_smapi(
    state: State<'_, AppState>,
    game_id: String,
) -> Result<manager_core::domain::SmapiInstallationRecord, String> {
    state.use_cases.install_smapi(&game_id, None)
}

// ---------------------------------------------------------------------------
// Launch & Session
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_launch_preflight(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<manager_core::launch::PreflightCheck, String> {
    let pid = ProfileId::from_str(&profile_id).map_err(|e| e.to_string())?;
    state
        .services
        .launch
        .get_launch_preflight(&pid, manager_core::launch::LaunchMode::Modded)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn launch_game(
    state: State<'_, AppState>,
    game_id: Option<String>,
    setup_id: Option<String>,
    profile_id: Option<String>,
) -> Result<LaunchSessionDto, String> {
    if let Some(ref pid_str) = profile_id {
        if let Ok(pid) = ProfileId::from_str(pid_str) {
            return state
                .services
                .launch
                .launch_profile(&pid, manager_core::launch::LaunchMode::Modded)
                .map_err(|e| e.to_string());
        }
    }

    if let Some(ref sid) = setup_id {
        let owned_by_legacy_stack =
            manager_core::ports::StateRepository::get_setup(&state.use_cases.repo, sid)
                .ok()
                .flatten()
                .is_some();
        if !owned_by_legacy_stack {
            if let Ok(pid) = ProfileId::from_str(sid) {
                return state
                    .services
                    .launch
                    .launch_profile(&pid, manager_core::launch::LaunchMode::Modded)
                    .map_err(|e| e.to_string());
            }
        }
    }

    let gid = game_id.ok_or("Game ID or valid profile ID required")?;
    let sid = setup_id.ok_or("Setup ID or valid profile ID required")?;
    if state.validate_setup(&sid)?.game_id != gid {
        return Err("Setup does not belong to game".into());
    }
    let mods_dir = state.paths.mods_dir(&sid);
    let session = state.use_cases.launch_game(&gid, &sid, &mods_dir)?;

    Ok(LaunchSessionDto {
        id: session.id,
        profile_id: session.setup_id,
        state: format!("{:?}", session.state).to_lowercase(),
        launched_at: session.launched_at.to_rfc3339(),
        ended_at: None,
        pid: session.pid,
        verified_mods: Vec::new(),
        verification_details: None,
    })
}

#[tauri::command]
pub fn get_session(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<LaunchSession>, String> {
    state.use_cases.repo.get_launch_session(&id)
}

#[tauri::command]
pub fn poll_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<LaunchSessionDto>, String> {
    if let Ok(sid) = LaunchSessionId::from_str(&session_id) {
        if let Ok(Some(dto)) = state.services.launch.poll_session(&sid) {
            return Ok(Some(dto));
        }
    }

    let session = state.use_cases.poll_session(&session_id)?;
    Ok(session.map(|s| LaunchSessionDto {
        id: s.id,
        profile_id: s.setup_id,
        state: format!("{:?}", s.state).to_lowercase(),
        launched_at: s.launched_at.to_rfc3339(),
        ended_at: None,
        pid: s.pid,
        verified_mods: Vec::new(),
        verification_details: None,
    }))
}

#[tauri::command]
pub fn terminate_game(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<(), String> {
    if let Some(ref sid_str) = session_id {
        if let Ok(sid) = LaunchSessionId::from_str(sid_str) {
            if state.services.launch.terminate_game(Some(&sid)).is_ok() {
                return Ok(());
            }
        }
    }
    state.use_cases.terminate_game(session_id.as_deref())
}

// ---------------------------------------------------------------------------
// Health & Diagnostics
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_health_summary(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<HealthSummaryDto, String> {
    let pid = match profile_id {
        Some(s) if !s.is_empty() => Some(ProfileId::from_str(&s).map_err(|e| e.to_string())?),
        _ => None,
    };
    state
        .services
        .health
        .get_health_summary(pid.as_ref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_diagnostics(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<DiagnosticsDto, String> {
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

#[tauri::command]
pub fn get_smapi_log(state: State<'_, AppState>) -> Result<String, String> {
    state
        .services
        .diagnostics
        .get_smapi_log()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_smapi_log_path(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state
        .services
        .diagnostics
        .get_smapi_log_path()
        .to_string_lossy()
        .to_string())
}

// ---------------------------------------------------------------------------
// Native Dialogs
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn pick_mod_file<R: tauri::Runtime>(
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

#[tauri::command]
pub async fn pick_game_directory<R: tauri::Runtime>(
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
pub async fn pick_folder_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    pick_game_directory(app).await
}

#[tauri::command]
pub async fn pick_archive_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    pick_mod_file(app).await
}

#[tauri::command]
pub fn list_game_installations(
    state: State<'_, AppState>,
) -> Result<Vec<GameInstallationSummaryDto>, String> {
    list_games(state)
}

#[tauri::command]
pub fn register_game_installation(
    state: State<'_, AppState>,
    path: String,
    storefront: Option<String>,
) -> Result<GameInstallationSummaryDto, String> {
    accept_game(state, path, storefront)
}

#[tauri::command]
pub fn validate_game_installation_path(
    state: State<'_, AppState>,
    path: String,
) -> Result<GameInspectionDto, String> {
    inspect_game_path(state, path)
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

#[tauri::command]
pub fn list_profile_mods(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<Vec<ModListItemDto>, String> {
    let pid_str = if let Some(id) = profile_id {
        id
    } else {
        let bootstrap = state
            .services
            .bootstrap
            .get_bootstrap()
            .map_err(|e| e.to_string())?;
        bootstrap
            .active_profile_id
            .ok_or_else(|| "No active profile".to_string())?
    };
    list_mods(state, pid_str)
}

#[tauri::command]
pub fn toggle_mod_enabled(
    state: State<'_, AppState>,
    profile_component_id: String,
    enabled: bool,
) -> Result<(), String> {
    toggle_mod(state, profile_component_id, enabled)
}

#[tauri::command]
pub fn inspect_package_for_install(
    state: State<'_, AppState>,
    archive_path: String,
    profile_id: Option<String>,
) -> Result<OperationPreviewDto, String> {
    let pid_str = if let Some(id) = profile_id {
        id
    } else {
        let bootstrap = state
            .services
            .bootstrap
            .get_bootstrap()
            .map_err(|e| e.to_string())?;
        bootstrap
            .active_profile_id
            .ok_or_else(|| "No active profile".to_string())?
    };
    prepare_install(state, pid_str, archive_path)
}

#[tauri::command]
pub fn execute_operation(
    state: State<'_, AppState>,
    operation_id: String,
) -> Result<OperationDto, String> {
    commit_operation(state, operation_id)
}

#[tauri::command]
pub fn list_recent_operations(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<OperationDto>, String> {
    list_operations(state, profile_id, limit)
}

#[tauri::command]
pub fn get_operation_details(
    state: State<'_, AppState>,
    operation_id: String,
) -> Result<Option<OperationDto>, String> {
    get_operation(state, None, Some(operation_id))
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
    let pid_str = if let Some(id) = profile_id {
        id
    } else {
        let bootstrap = state
            .services
            .bootstrap
            .get_bootstrap()
            .map_err(|e| e.to_string())?;
        bootstrap
            .active_profile_id
            .ok_or_else(|| "No active profile".to_string())?
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
    terminate_game(state, session_id)
}

#[tauri::command]
pub fn get_diagnostics_report(
    state: State<'_, AppState>,
    game_installation_id: Option<String>,
    session_id: Option<String>,
) -> Result<DiagnosticsDto, String> {
    let _ = game_installation_id;
    get_diagnostics(state, session_id)
}

// ---------------------------------------------------------------------------
// Legacy Mod Inspection & Install (Backward Compatibility)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn inspect_mod(
    state: State<'_, AppState>,
    file_path: String,
    setup_id: String,
) -> Result<ArchiveInspectionResult, String> {
    state.validate_setup(&setup_id)?;
    let zip_path = resolve_mod_file_path(&file_path)?;

    let staging_dir = state.paths.staging_dir(&setup_id, "inspect");
    let result = SafeZipExtractor::inspect_and_stage(
        &zip_path,
        &setup_id,
        &staging_dir,
        &state.use_cases.repo,
    )?;
    state.pending_plans.insert_staged(
        result.plan.clone(),
        Some(staging_dir.join(&result.plan.plan_id)),
    );
    Ok(result)
}

#[tauri::command]
pub fn install_mod(state: State<'_, AppState>, plan_id: String) -> Result<InstalledMod, String> {
    let plan = state.pending_plans.take(&plan_id).ok_or_else(|| {
        format!(
            "Pending install plan '{}' not found or expired. Please re-inspect the mod archive.",
            plan_id
        )
    })?;
    state.validate_setup(&plan.setup_id)?;
    let staging_dir = state.paths.staging_dir(&plan.setup_id, "inspect");
    let final_mods_dir = state.paths.mods_dir(&plan.setup_id);
    let result = state
        .use_cases
        .commit_mod_install(&plan, &staging_dir, &final_mods_dir);
    if result.is_err() {
        let owned_by_journal = state
            .use_cases
            .repo
            .list_unresolved_operations()?
            .iter()
            .any(|op| {
                serde_json::from_str::<manager_core::install::InstallPlan>(&op.plan_json)
                    .is_ok_and(|p| p.plan_id == plan.plan_id)
            });
        if !owned_by_journal {
            let _ = std::fs::remove_dir_all(staging_dir.join(&plan.plan_id));
        }
    }
    result
}

#[tauri::command]
pub fn remove_mod(
    state: State<'_, AppState>,
    installed_mod_id: String,
    setup_id: String,
) -> Result<(), String> {
    state.validate_setup(&setup_id)?;
    let item = state
        .use_cases
        .repo
        .get_installed_mod(&installed_mod_id)?
        .ok_or("Mod not found")?;
    if item.setup_id != setup_id {
        return Err("Mod does not belong to setup".into());
    }
    manager_core::install::validate_relative_path(&item.relative_target_path)?;
    let mods_dir = state.paths.mods_dir(&setup_id);
    let recovery_dir = state.paths.recovery_dir(&setup_id, "remove");
    state
        .use_cases
        .remove_mod(&installed_mod_id, &setup_id, &mods_dir, &recovery_dir)
}

#[tauri::command]
pub fn cancel_inspection(state: State<'_, AppState>, plan_id: String) -> Result<(), String> {
    state.pending_plans.remove(&plan_id);
    Ok(())
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
