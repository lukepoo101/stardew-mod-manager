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
    unmanaged: Option<bool>,
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
                if unmanaged.unwrap_or(false) {
                    manager_core::game::ManagementMode::ExternalUnmanaged
                } else {
                    manager_core::game::ManagementMode::Managed
                },
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

/// Marks a profile as the game's default, or clears the default when
/// `profile_id` is absent. The active profile is left as it is.
#[tauri::command]
pub fn set_default_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> IpcResult<()> {
    let gid = active_game_id(&state, None).into_ipc()?;
    let pid = profile_id
        .map(|id| ProfileId::from_str(&id).map_err(ipc::invalid_profile_id))
        .transpose()
        .into_ipc()?;
    events::after_state_change(&app, || {
        state
            .services
            .profiles
            .set_default_profile(&gid, pid.as_ref())
            .into_ipc()
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
    let mut mods = state.mods_queries.list_profile_mods(&pid).into_ipc()?;
    // A cheap check per mod: is its folder where the manager put it?
    use manager_app::ports::repositories::DeploymentRepository;
    let live = state.paths.profile_mods_dir(&pid);
    let disabled = state.paths.profile_disabled_dir(&pid);
    for item in &mut mods {
        let Ok(deployment_id) = DeploymentId::from_str(&item.deployment_id) else {
            continue;
        };
        if let Ok(Some(deployment)) = state.repo.get_deployment(&deployment_id) {
            let relative = Path::new(&deployment.root_relative_path);
            item.folder_missing =
                !live.join(relative).exists() && !disabled.join(relative).exists();
        }
    }
    Ok(mods)
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
    previous: Option<manager_app::api::dto::DismissedSnapshotDto>,
) -> IpcResult<()> {
    events::after_state_change(&app, || {
        manager_app::services::FindingDismissals::new(state.repo.clone())
            .dismiss_noting(&fingerprint, &signature, &severity, previous)
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
    settings_for: Option<Vec<String>>,
    optional: Option<Vec<String>>,
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
        .export_bundle_with(
            &pid,
            std::path::Path::new(&destination_dir),
            &settings_for.unwrap_or_default(),
            &optional.unwrap_or_default(),
        )
        .into_ipc()
}

/// Mods in a profile that have settings files, with what sharing them might
/// reveal.
#[tauri::command]
pub fn list_shareable_settings(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Vec<ShareableSettingsDto>> {
    use manager_app::ports::deployed_files::DeployedFilesPort;
    use manager_app::ports::repositories::{DeploymentRepository, PackageCatalogRepository};
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    let files = manager_infra::deployed_files::FilesystemDeployedFiles::new(state.paths.clone());
    let mut out = Vec::new();
    for pc in state.repo.list_profile_components(&pid).into_ipc()? {
        let Some(component) = state
            .repo
            .get_package_component(&pc.package_component_id)
            .into_ipc()?
        else {
            continue;
        };
        let Some(deployment) = state.repo.get_deployment(&pc.deployment_id).into_ipc()? else {
            continue;
        };
        let configs = files
            .read_configs(&pid, &deployment.root_relative_path)
            .into_ipc()?;
        if configs.is_empty() {
            continue;
        }
        let mut warnings = Vec::new();
        for (path, bytes) in &configs {
            warnings.extend(
                manager_core::settings_privacy::settings_warnings(bytes)
                    .into_iter()
                    .map(|w| format!("{path}: {w}")),
            );
        }
        out.push(ShareableSettingsDto {
            unique_id: component.unique_id.to_string(),
            name: component.name,
            files: configs.into_iter().map(|(path, _)| path).collect(),
            warnings,
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    Ok(out)
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
    include_optional: Option<Vec<String>>,
) -> IpcResult<manager_app::api::dto::BundleImportDto> {
    let resolved = resolve_mod_file_path(&bundle_path).into_ipc()?;
    let gid = GameInstallationId::from_str(&game_id)
        .map_err(ipc::invalid_game_installation_id)
        .into_ipc()?;
    let bundle = state.services.bundle.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        bundle.import_bundle_choosing(&resolved, &gid, &profile_name, include_optional.as_deref())
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

/// Closes an operation waiting for recovery that the user resolved by hand.
#[tauri::command]
pub fn mark_operation_handled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    operation_id: String,
) -> IpcResult<()> {
    let id = OperationId::from_str(&operation_id)
        .map_err(ipc::invalid_operation_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.operations.mark_handled(&id).into_ipc()
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
    acknowledged_warnings: Option<Vec<String>>,
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
        state
            .services
            .launch
            .launch_profile_acknowledging(&pid, mode, acknowledged_warnings.as_deref())
            .into_ipc()
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

/// Lets the user choose several mod archives at once, for a batch install.
#[tauri::command]
pub async fn pick_archives_dialog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> IpcResult<Vec<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Select Stardew Valley Mod ZIPs")
        .add_filter("ZIP Archives", &["zip"])
        .pick_files(move |files| {
            let _ = tx.send(
                files
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>(),
            );
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

/// How much recovery data storage cleanup keeps.
#[tauri::command]
pub fn get_retention_policy(state: State<'_, AppState>) -> IpcResult<RetentionPolicyDto> {
    state.services.storage.retention().into_ipc()
}

/// Changes how much recovery data cleanup keeps. Nothing is removed until a
/// cleanup is run.
#[tauri::command]
pub fn set_retention_policy(
    state: State<'_, AppState>,
    policy: RetentionPolicyDto,
) -> IpcResult<RetentionPolicyDto> {
    state.services.storage.set_retention(policy).into_ipc()
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

/// Sets or clears a source link the user supplies for a mod.
#[tauri::command]
pub fn set_mod_source_link<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    unique_id: String,
    url: Option<String>,
) -> IpcResult<ModAnnotationDto> {
    events::after_state_change(&app, || {
        manager_app::services::ModAnnotations::new(state.repo.clone())
            .set_source(&unique_id, url.as_deref())
            .into_ipc()
    })
}

/// Renames a tag on every mod; renaming onto an existing tag merges them.
#[tauri::command]
pub fn rename_mod_tag<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> IpcResult<usize> {
    events::after_state_change(&app, || {
        manager_app::services::ModAnnotations::new(state.repo.clone())
            .rename_tag(&from, &to)
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

/// The files an install put in a mod's folder, the other mods from the same
/// package, and whether that package is still stored intact.
#[tauri::command]
pub fn get_mod_package_files(
    state: State<'_, AppState>,
    profile_component_id: String,
) -> IpcResult<manager_app::api::dto::ModPackageFilesDto> {
    use manager_app::ports::repositories::{DeploymentRepository, PackageCatalogRepository};
    let (component, deployment) = mod_record(&state, &profile_component_id).into_ipc()?;
    let files = file_integrity(&state)
        .installed_files(&component.profile_id, &deployment.id.to_string())
        .into_ipc()?;
    let mut package_mods = Vec::new();
    for pc in state
        .repo
        .list_profile_components(&component.profile_id)
        .into_ipc()?
    {
        if pc.deployment_id != deployment.id {
            continue;
        }
        if let Some(comp) = state
            .repo
            .get_package_component(&pc.package_component_id)
            .into_ipc()?
        {
            package_mods.push(format!("{} {}", comp.name, comp.version));
        }
    }
    package_mods.sort();
    let hash = &deployment.artifact_hash;
    let package_stored = state.services.packages.has_artifact(hash);
    let package_intact = if package_stored {
        Some(
            state
                .services
                .packages
                .verify_artifact(hash)
                .unwrap_or(false),
        )
    } else {
        None
    };
    Ok(manager_app::api::dto::ModPackageFilesDto {
        recorded: files.is_some(),
        files: files
            .unwrap_or_default()
            .into_iter()
            .map(|e| manager_app::api::dto::PlanFileDto {
                path: e.relative_path,
                size_bytes: e.size_bytes,
                installed: true,
            })
            .collect(),
        package_mods,
        artifact_hash: hash.as_str().to_string(),
        package_stored,
        package_intact,
    })
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

/// The space a mod's folder and its stored archive take, measured on disk.
#[tauri::command]
pub fn get_mod_size(
    state: State<'_, AppState>,
    profile_component_id: String,
) -> IpcResult<ModSizeDto> {
    let folder_bytes = mod_files_path(&state, &profile_component_id)
        .ok()
        .and_then(|path| manager_infra::storage_usage::size(&path));
    let archive_bytes = mod_package_path(&state, &profile_component_id)
        .ok()
        .and_then(|path| manager_infra::storage_usage::size(&path));
    Ok(ModSizeDto {
        folder_bytes,
        archive_bytes,
    })
}

/// The places the manager and the game use, by id.
fn locations(state: &State<'_, AppState>) -> Vec<(String, String, Option<PathBuf>, String)> {
    use manager_app::ports::repositories::GameInstallationRepository;
    let game = active_game_id(state, None)
        .ok()
        .and_then(|gid| state.repo.get_game(&gid).ok().flatten());
    let profile = state
        .services
        .bootstrap
        .get_bootstrap()
        .ok()
        .and_then(|b| b.active_profile_id)
        .and_then(|id| ProfileId::from_str(&id).ok());
    let data = state.paths.data_dir().to_path_buf();
    let saves = state
        .services
        .saves
        .list()
        .ok()
        .and_then(|s| s.saves_dir)
        .map(PathBuf::from);
    vec![
        (
            "game".into(),
            "Game folder".into(),
            game.map(|g| g.canonical_root),
            "Your Stardew Valley installation. The manager only changes it to install or repair SMAPI.".into(),
        ),
        (
            "profile_mods".into(),
            "This profile's Mods folder".into(),
            profile.map(|p| state.paths.profile_mods_dir(&p)),
            "The mods SMAPI loads for the active profile. Change it through the manager, not by hand.".into(),
        ),
        (
            "saves".into(),
            "Save folder".into(),
            saves,
            "Your farms, managed by the game. The manager only reads them, apart from restoring a backup you choose.".into(),
        ),
        (
            "data".into(),
            "Manager data".into(),
            Some(data.clone()),
            "Profiles, records and recovery data. Do not delete it: your profiles and undo history live here.".into(),
        ),
        (
            "packages".into(),
            "Stored mod archives".into(),
            Some(state.paths.packages_dir()),
            "Archives mods were installed from, used to reinstall, restore and share. Storage cleanup removes the ones nothing needs.".into(),
        ),
        (
            "backups".into(),
            "Save and settings backups".into(),
            Some(data.join("save-backups")),
            "Backups you or the manager made. Storage cleanup keeps the newest of each; settings backups are in config-backups beside it.".into(),
        ),
        (
            "trash".into(),
            "Deleted profiles".into(),
            Some(data.join("trash")),
            "Folders of deleted profiles, kept for a while in case you want them back.".into(),
        ),
        (
            "cache".into(),
            "Cache".into(),
            Some(state.paths.cache_dir().to_path_buf()),
            "Downloaded SMAPI installers. Safe to clear: they are downloaded again when needed.".into(),
        ),
    ]
}

#[tauri::command]
pub fn get_locations(state: State<'_, AppState>) -> IpcResult<Vec<LocationDto>> {
    Ok(locations(&state)
        .into_iter()
        .map(|(id, label, path, note)| LocationDto {
            exists: path.as_ref().is_some_and(|p| p.exists()),
            path: path.map(|p| p.to_string_lossy().to_string()),
            id,
            label,
            note,
        })
        .collect())
}

/// Opens one of the places from `get_locations` in the file manager.
#[tauri::command]
pub fn reveal_location(state: State<'_, AppState>, id: String) -> IpcResult<()> {
    let path = locations(&state)
        .into_iter()
        .find(|(known, ..)| known == &id)
        .and_then(|(_, _, path, _)| path)
        .filter(|path| path.exists())
        .ok_or_else(|| {
            manager_app::error::AppError::validation(
                "LOCATION_UNAVAILABLE",
                "That folder is not there on this computer",
            )
        });
    reveal(path)
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

#[tauri::command]
pub fn get_mod_relations(
    state: State<'_, AppState>,
    profile_component_id: String,
) -> IpcResult<Option<ModRelationsDto>> {
    let cid = ProfileComponentId::from_str(&profile_component_id)
        .map_err(ipc::invalid_profile_component_id)
        .into_ipc()?;
    state.mods_queries.get_mod_relations(&cid).into_ipc()
}

/// Every mod in a profile with what it needs and what needs it.
#[tauri::command]
pub fn get_dependency_map(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Vec<manager_app::api::dto::DependencyMapEntryDto>> {
    let pid = parse_profile_id(&profile_id)?;
    state.mods_queries.get_dependency_map(&pid).into_ipc()
}

/// The most recent launch of the active profile, running or finished, with
/// its state refreshed from the process.
#[tauri::command]
pub fn get_latest_launch_session(
    state: State<'_, AppState>,
) -> IpcResult<Option<LaunchSessionDto>> {
    let bootstrap = state.services.bootstrap.get_bootstrap().into_ipc()?;
    let Some(pid) = bootstrap.active_profile_id else {
        return Ok(None);
    };
    let pid = ProfileId::from_str(&pid)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    let Some(session) = state
        .services
        .launch
        .get_latest_session(Some(&pid))
        .into_ipc()?
    else {
        return Ok(None);
    };
    let id = LaunchSessionId::from_str(&session.id)
        .map_err(ipc::invalid_launch_session_id)
        .into_ipc()?;
    state.services.launch.poll_session(&id).into_ipc()
}

/// The active profile's recent game sessions, newest first.
#[tauri::command]
pub fn list_launch_sessions(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> IpcResult<Vec<LaunchSessionDto>> {
    let bootstrap = state.services.bootstrap.get_bootstrap().into_ipc()?;
    let Some(pid) = bootstrap.active_profile_id else {
        return Ok(Vec::new());
    };
    let pid = ProfileId::from_str(&pid)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    state
        .services
        .launch
        .recent_sessions(&pid, limit.unwrap_or(20).min(100))
        .into_ipc()
}

#[tauri::command]
pub fn preview_profile_deletion(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<ProfileDeletePreviewDto> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    state.services.profile_deletion.preview(&pid).into_ipc()
}

#[tauri::command]
pub fn delete_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<()> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.profile_deletion.delete(&pid).into_ipc()
    })
}

fn profile_freeze(state: &State<'_, AppState>) -> manager_app::services::ProfileFreeze {
    manager_app::services::ProfileFreeze::new(
        state.repo.clone(),
        state.repo.clone(),
        state.repo.clone(),
        state.repo.clone(),
    )
}

#[tauri::command]
pub fn get_profile_freeze(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Option<ProfileFreezeDto>> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    profile_freeze(&state).status(&pid).into_ipc()
}

#[tauri::command]
pub fn freeze_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    reason: String,
    game_version: Option<String>,
    smapi_version: Option<String>,
) -> IpcResult<ProfileFreezeDto> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    // The versions are the latest the app observed, kept as context only.
    let settings = shared_settings(&state).hashes(&pid).into_ipc()?;
    events::after_state_change(&app, || {
        profile_freeze(&state)
            .freeze_with(
                &pid,
                &reason,
                manager_app::services::FreezeContext {
                    settings,
                    game_version,
                    smapi_version,
                },
            )
            .into_ipc()
    })
}

#[tauri::command]
pub fn unfreeze_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<()> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || profile_freeze(&state).unfreeze(&pid).into_ipc())
}

#[tauri::command]
pub fn clone_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    name: String,
) -> IpcResult<BundleImportDto> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state.services.bundle.clone_profile(&pid, &name).into_ipc()
    })
}

/// How the settings of the mods two profiles share compare.
#[tauri::command]
pub fn compare_profile_settings(
    state: State<'_, AppState>,
    first_profile_id: String,
    second_profile_id: String,
) -> IpcResult<Vec<SettingsComparisonDto>> {
    let first = ProfileId::from_str(&first_profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    let second = ProfileId::from_str(&second_profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    state
        .services
        .bundle
        .compare_settings(&first, &second)
        .into_ipc()
}

/// Builds a new profile from a restore point, or from the last working setup
/// with point id `known-good`.
#[tauri::command]
pub fn recreate_profile_from_point<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    point_id: String,
    name: String,
) -> IpcResult<BundleImportDto> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        state
            .services
            .bundle
            .recreate_from_point(&pid, &point_id, &name)
            .into_ipc()
    })
}

/// Deleted profiles of the active game that can still be brought back.
#[tauri::command]
pub fn list_deleted_profiles(state: State<'_, AppState>) -> IpcResult<Vec<DeletedProfileDto>> {
    let gid = active_game_id(&state, None).into_ipc()?;
    state.services.bundle.deleted_profiles(&gid).into_ipc()
}

/// Recreates a deleted profile from its record in the trash.
#[tauri::command]
pub fn bring_back_profile<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    entry: String,
) -> IpcResult<BundleImportDto> {
    events::after_state_change(&app, || state.services.bundle.bring_back(&entry).into_ipc())
}

/// Duplicates of the active game's profiles that were started but not
/// finished.
#[tauri::command]
pub fn list_unfinished_copies(state: State<'_, AppState>) -> IpcResult<Vec<UnfinishedCopyDto>> {
    let gid = active_game_id(&state, None).into_ipc()?;
    state.services.bundle.unfinished_copies(&gid).into_ipc()
}

/// Completes an interrupted duplicate from what was recorded when it started.
#[tauri::command]
pub fn finish_profile_copy<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<BundleImportDto> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || state.services.bundle.finish_copy(&pid).into_ipc())
}

fn experiments(state: &State<'_, AppState>) -> manager_app::services::ProfileExperiments {
    manager_app::services::ProfileExperiments::new(state.repo.clone(), state.repo.clone())
}

#[tauri::command]
pub fn list_experiments(state: State<'_, AppState>) -> IpcResult<Vec<ExperimentDto>> {
    experiments(&state).list().into_ipc()
}

/// Copies a profile into a new experiment and marks where it came from. The
/// source is not changed; activating the experiment is a separate step.
#[tauri::command]
pub fn start_experiment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    source_profile_id: String,
    name: String,
) -> IpcResult<BundleImportDto> {
    let source = ProfileId::from_str(&source_profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        let copy = state
            .services
            .bundle
            .clone_profile(&source, &name)
            .into_ipc()?;
        let experiment = ProfileId::from_str(&copy.profile_id)
            .map_err(ipc::invalid_profile_id)
            .into_ipc()?;
        experiments(&state).mark(&experiment, &source).into_ipc()?;
        Ok(copy)
    })
}

#[tauri::command]
pub fn keep_experiment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<()> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || experiments(&state).unmark(&pid).into_ipc())
}

// ---------------------------------------------------------------------------
// Saves
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_saves(state: State<'_, AppState>) -> IpcResult<SavesDto> {
    state.services.saves.list().into_ipc()
}

#[tauri::command]
pub fn associate_save<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    save_id: String,
    profile_id: Option<String>,
) -> IpcResult<()> {
    let pid = profile_id
        .map(|id| ProfileId::from_str(&id).map_err(ipc::invalid_profile_id))
        .transpose()
        .into_ipc()?;
    events::after_state_change(&app, || {
        state
            .services
            .saves
            .associate(&save_id, pid.as_ref())
            .into_ipc()
    })
}

#[tauri::command]
pub fn backup_save<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    save_id: String,
    note: Option<String>,
) -> IpcResult<SaveBackupDto> {
    events::after_state_change(&app, || {
        state
            .services
            .saves
            .backup_noting(&save_id, note.as_deref())
            .into_ipc()
    })
}

#[tauri::command]
pub fn restore_save_backup<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    backup_id: String,
) -> IpcResult<SaveBackupDto> {
    events::after_state_change(&app, || state.services.saves.restore(&backup_id).into_ipc())
}

#[tauri::command]
pub fn get_known_good(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Option<KnownGoodDto>> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    manager_app::services::KnownGood::new(
        state.repo.clone(),
        state.repo.clone(),
        state.repo.clone(),
    )
    .get(&pid)
    .into_ipc()
}

/// Forgets a profile's last working setup, after the user confirmed it.
#[tauri::command]
pub fn forget_known_good<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<()> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        manager_app::services::KnownGood::new(
            state.repo.clone(),
            state.repo.clone(),
            state.repo.clone(),
        )
        .forget(&pid)
        .into_ipc()
    })
}

#[tauri::command]
pub fn check_mod_files(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Vec<ModFilesCheckDto>> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    file_integrity(&state).check_profile(&pid).into_ipc()
}

fn shared_settings(state: &State<'_, AppState>) -> manager_app::services::SharedSettingsService {
    manager_app::services::SharedSettingsService::new(
        state.repo.clone(),
        state.repo.clone(),
        state.repo.clone(),
        std::sync::Arc::new(manager_infra::deployed_files::FilesystemDeployedFiles::new(
            state.paths.clone(),
        )),
        std::sync::Arc::new(manager_infra::config_backups::FilesystemConfigBackups::new(
            &state.paths,
        )),
    )
}

/// The chosen mods' settings files as text, for sharing in a recipe.
#[tauri::command]
pub fn read_shared_settings(
    state: State<'_, AppState>,
    profile_id: String,
    unique_ids: Vec<String>,
) -> IpcResult<Vec<manager_app::api::dto::SharedSettingsDto>> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    shared_settings(&state)
        .read_for_sharing(&pid, &unique_ids)
        .into_ipc()
}

/// Checksums of every mod's settings files; never their contents.
#[tauri::command]
pub fn settings_hashes(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Vec<manager_app::api::dto::SettingFileHashDto>> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    shared_settings(&state).hashes(&pid).into_ipc()
}

/// Writes a recipe's settings into a mod's folder after backing up the
/// current ones. Returns the backup id, if anything was backed up.
#[tauri::command]
pub fn apply_shared_settings<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    unique_id: String,
    settings: Vec<manager_core::recipe::RecipeSetting>,
) -> IpcResult<Option<String>> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        shared_settings(&state)
            .apply(&pid, &unique_id, &settings)
            .into_ipc()
    })
}

/// Compares mod folders with their install records by file size only,
/// without reading contents, so it can run without being asked.
#[tauri::command]
pub fn quick_check_mod_files(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Vec<ModFilesCheckDto>> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    file_integrity(&state).quick_check_profile(&pid).into_ipc()
}

fn file_integrity(state: &State<'_, AppState>) -> manager_app::services::FileIntegrityService {
    manager_app::services::FileIntegrityService::new(
        state.repo.clone(),
        state.repo.clone(),
        state.repo.clone(),
        std::sync::Arc::new(manager_infra::deployed_files::FilesystemDeployedFiles::new(
            state.paths.clone(),
        )),
    )
    .with_preferences(state.repo.clone())
}

/// Accepts a mod folder's changed files as they are now; nothing is touched.
#[tauri::command]
pub fn accept_mod_files<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    deployment_id: String,
) -> IpcResult<ModFilesCheckDto> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        file_integrity(&state)
            .accept_current(&pid, &deployment_id)
            .into_ipc()
    })
}

/// Opens one of the few external pages the manager links to.
#[tauri::command]
pub fn open_external_page(url: String) -> IpcResult<()> {
    manager_infra::reveal::open_known_page(&url)
        .map_err(|error| {
            manager_app::error::AppError::validation(
                "PAGE_NOT_OPENED",
                "That page could not be opened in the browser",
            )
            .with_details(error.to_string())
        })
        .into_ipc()
}

#[tauri::command]
pub fn get_operation_history_details(
    state: State<'_, AppState>,
    operation_id: String,
) -> IpcResult<Option<OperationDetailsDto>> {
    let id = OperationId::from_str(&operation_id)
        .map_err(|_| {
            manager_app::error::AppError::validation(
                "OPERATION_ID_INVALID",
                "That operation id is not valid",
            )
        })
        .into_ipc()?;
    state.services.operations.operation_details(&id).into_ipc()
}

fn reference_recipes(state: &State<'_, AppState>) -> manager_app::services::ReferenceRecipes {
    manager_app::services::ReferenceRecipes::new(state.repo.clone())
}

fn parse_profile_id(profile_id: &str) -> IpcResult<ProfileId> {
    ProfileId::from_str(profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()
}

#[tauri::command]
pub fn get_reference_recipe(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Option<ReferenceRecipeDto>> {
    reference_recipes(&state)
        .get(&parse_profile_id(&profile_id)?)
        .into_ipc()
}

#[tauri::command]
pub fn attach_reference_recipe<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    recipe_json: String,
) -> IpcResult<ReferenceRecipeDto> {
    let pid = parse_profile_id(&profile_id)?;
    events::after_state_change(&app, || {
        reference_recipes(&state)
            .attach(&pid, &recipe_json)
            .into_ipc()
    })
}

#[tauri::command]
pub fn set_reference_difference_accepted<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    difference_key: String,
    accepted: bool,
) -> IpcResult<ReferenceRecipeDto> {
    let pid = parse_profile_id(&profile_id)?;
    events::after_state_change(&app, || {
        reference_recipes(&state)
            .set_accepted(&pid, &difference_key, accepted)
            .into_ipc()
    })
}

#[tauri::command]
pub fn detach_reference_recipe<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<()> {
    let pid = parse_profile_id(&profile_id)?;
    events::after_state_change(&app, || reference_recipes(&state).detach(&pid).into_ipc())
}

/// Reinstall and replace as the user starts them: settings are backed up and
/// a restore point is saved first.
fn reinstall_service(state: &State<'_, AppState>) -> manager_app::services::ReinstallService {
    reinstall_service_without_snapshots(state)
        .with_restore_points(state.repo.clone(), state.repo.clone())
}

/// For a restore, which saves its own undo point once for the whole change.
fn reinstall_service_without_snapshots(
    state: &State<'_, AppState>,
) -> manager_app::services::ReinstallService {
    manager_app::services::ReinstallService::new(
        state.repo.clone(),
        state.services.packages.clone(),
        state.services.mods.clone(),
        state.services.operations.clone(),
        state.services.toggle.clone(),
        std::sync::Arc::new(manager_infra::deployed_files::FilesystemDeployedFiles::new(
            state.paths.clone(),
        )),
    )
    .with_config_backups(
        std::sync::Arc::new(manager_infra::config_backups::FilesystemConfigBackups::new(
            &state.paths,
        )),
        state.repo.clone(),
    )
}

#[tauri::command]
pub fn reinstall_mod<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_component_id: String,
) -> IpcResult<ReinstallResultDto> {
    let cid = ProfileComponentId::from_str(&profile_component_id)
        .map_err(ipc::invalid_profile_component_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        reinstall_service(&state).reinstall(&cid).into_ipc()
    })
}

#[tauri::command]
pub fn replace_mod_version<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    artifact_hash: String,
) -> IpcResult<ReplaceResultDto> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        reinstall_service(&state)
            .replace(&pid, &artifact_hash)
            .into_ipc()
    })
}

/// Installs a package the manager already stores, such as one a shared
/// recipe asks for.
#[tauri::command]
pub fn install_stored_package<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    artifact_hash: String,
    as_dependency: Option<bool>,
) -> IpcResult<()> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    events::after_state_change(&app, || {
        let service = reinstall_service(&state);
        if as_dependency.unwrap_or(false) {
            service.install_stored_as_dependency(&pid, &artifact_hash)
        } else {
            service.install_stored(&pid, &artifact_hash)
        }
        .into_ipc()
    })
}

/// Stored packages that provide a mod with this UniqueID, for installing a
/// missing dependency without downloading anything.
#[tauri::command]
pub fn find_stored_mod(
    state: State<'_, AppState>,
    unique_id: String,
    minimum_version: Option<String>,
) -> IpcResult<Vec<manager_app::api::dto::StoredCandidateDto>> {
    state
        .services
        .packages
        .stored_with_unique_id(&unique_id, minimum_version.as_deref())
        .into_ipc()
}

/// Which of these package checksums the manager stores intact.
#[tauri::command]
pub fn stored_packages(
    state: State<'_, AppState>,
    artifact_hashes: Vec<String>,
) -> IpcResult<Vec<String>> {
    let mut stored = Vec::new();
    for raw in artifact_hashes.iter().take(5000) {
        let Ok(hash) = ArtifactHash::parse(raw.to_lowercase()) else {
            continue;
        };
        if state.services.packages.has_artifact(&hash) {
            stored.push(raw.clone());
        }
    }
    Ok(stored)
}

#[tauri::command]
pub fn get_storage_usage(state: State<'_, AppState>) -> IpcResult<StorageUsageDto> {
    use manager_app::ports::repositories::{GameInstallationRepository, ProfileRepository};
    use manager_app::ports::storage_usage::StorageUsagePort;
    let usage = manager_infra::storage_usage::FilesystemStorageUsage::new(state.paths.clone());
    let mut profiles = Vec::new();
    for game in state.repo.list_games().into_ipc()? {
        for profile in state.repo.list_profiles(&game.id).into_ipc()? {
            let measured = usage.profile(&profile.id);
            profiles.push(ProfileStorageDto {
                profile_id: profile.id.to_string(),
                name: profile.name,
                archived: profile.state == manager_core::profile::ProfileState::Archived,
                live_bytes: measured.live,
                disabled_bytes: measured.disabled,
                operations_bytes: measured.operations,
            });
        }
    }
    let areas = usage.areas();
    Ok(StorageUsageDto {
        profiles,
        packages_bytes: areas.packages,
        installer_cache_bytes: areas.installer_cache,
        save_backups_bytes: areas.save_backups,
        trash_bytes: areas.trash,
    })
}

/// The profile, UniqueID and folder of a mod, for its settings backups.
fn mod_settings_target(
    state: &State<'_, AppState>,
    profile_component_id: &str,
) -> AppResult<(ProfileId, String, String)> {
    use manager_app::ports::repositories::PackageCatalogRepository;
    let (component, deployment) = mod_record(state, profile_component_id)?;
    let unique_id = state
        .repo
        .get_package_component(&component.package_component_id)?
        .map(|c| c.unique_id.to_string())
        .ok_or_else(|| {
            manager_app::error::AppError::validation(
                "COMPONENT_NOT_FOUND",
                "That mod has no record",
            )
        })?;
    Ok((
        component.profile_id,
        unique_id,
        deployment.root_relative_path,
    ))
}

#[tauri::command]
pub fn list_config_backups(
    state: State<'_, AppState>,
    profile_component_id: String,
) -> IpcResult<Vec<ConfigBackupDto>> {
    use manager_app::ports::config_backups::ConfigBackupsPort;
    let (profile_id, unique_id, _) =
        mod_settings_target(&state, &profile_component_id).into_ipc()?;
    let backups = manager_infra::config_backups::FilesystemConfigBackups::new(&state.paths);
    Ok(backups
        .list(&profile_id, &unique_id)
        .into_ipc()?
        .into_iter()
        .map(|b| ConfigBackupDto {
            id: b.id,
            created_at: b.created_at.to_rfc3339(),
            files: b.files,
        })
        .collect())
}

/// Writes a settings backup back into the mod's current folder.
#[tauri::command]
pub fn restore_config_backup<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_component_id: String,
    backup_id: String,
) -> IpcResult<()> {
    use manager_app::ports::config_backups::ConfigBackupsPort;
    use manager_app::ports::deployed_files::DeployedFilesPort;
    let (profile_id, unique_id, folder) =
        mod_settings_target(&state, &profile_component_id).into_ipc()?;
    if !backup_id
        .to_lowercase()
        .starts_with(&format!("{}/", unique_id.to_lowercase()))
    {
        return Err(manager_app::error::AppError::validation(
            "BACKUP_NOT_FOR_MOD",
            "That backup belongs to a different mod",
        ))
        .into_ipc();
    }
    events::after_state_change(&app, || {
        let files = manager_infra::config_backups::FilesystemConfigBackups::new(&state.paths)
            .load(&profile_id, &backup_id)
            .into_ipc()?;
        manager_infra::deployed_files::FilesystemDeployedFiles::new(state.paths.clone())
            .write_files(&profile_id, &folder, &files)
            .into_ipc()
    })
}

#[tauri::command]
pub fn list_mod_problems(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Vec<ModProblemDto>> {
    let pid = ProfileId::from_str(&profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()?;
    state.mods_queries.profile_problems(&pid).into_ipc()
}

fn restore_points(state: &State<'_, AppState>) -> manager_app::services::RestorePoints {
    manager_app::services::RestorePoints::new(
        state.repo.clone(),
        state.repo.clone(),
        state.repo.clone(),
        state.services.packages.clone(),
        state.services.mods.clone(),
        state.services.operations.clone(),
        state.services.toggle.clone(),
        std::sync::Arc::new(reinstall_service_without_snapshots(state)),
    )
    .with_settings(
        std::sync::Arc::new(manager_infra::deployed_files::FilesystemDeployedFiles::new(
            state.paths.clone(),
        )),
        std::sync::Arc::new(manager_infra::config_backups::FilesystemConfigBackups::new(
            &state.paths,
        )),
    )
}

fn restore_profile_id(profile_id: &str) -> IpcResult<ProfileId> {
    ProfileId::from_str(profile_id)
        .map_err(ipc::invalid_profile_id)
        .into_ipc()
}

#[tauri::command]
pub fn list_restore_points(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Vec<RestorePointDto>> {
    restore_points(&state)
        .list(&restore_profile_id(&profile_id)?)
        .into_ipc()
}

#[tauri::command]
pub fn create_restore_point<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    label: String,
) -> IpcResult<RestorePointDto> {
    let pid = restore_profile_id(&profile_id)?;
    events::after_state_change(&app, || {
        restore_points(&state).create(&pid, &label).into_ipc()
    })
}

#[tauri::command]
pub fn delete_restore_point<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    point_id: String,
) -> IpcResult<()> {
    let pid = restore_profile_id(&profile_id)?;
    events::after_state_change(&app, || {
        restore_points(&state).delete(&pid, &point_id).into_ipc()
    })
}

#[tauri::command]
pub fn plan_restore(
    state: State<'_, AppState>,
    profile_id: String,
    point_id: String,
) -> IpcResult<RestorePlanDto> {
    restore_points(&state)
        .plan(&restore_profile_id(&profile_id)?, &point_id)
        .into_ipc()
}

#[tauri::command]
pub fn restore_to_point<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    profile_id: String,
    point_id: String,
) -> IpcResult<RestoreResultDto> {
    let pid = restore_profile_id(&profile_id)?;
    events::after_state_change(&app, || {
        restore_points(&state).restore(&pid, &point_id).into_ipc()
    })
}

fn collections(state: &State<'_, AppState>) -> manager_app::services::Collections {
    manager_app::services::Collections::new(state.repo.clone())
}

/// The curator's saved collection draft for a profile.
#[tauri::command]
pub fn get_collection_draft(
    state: State<'_, AppState>,
    profile_id: String,
) -> IpcResult<Option<String>> {
    let pid = parse_profile_id(&profile_id)?;
    collections(&state).draft(&pid).into_ipc()
}

#[tauri::command]
pub fn save_collection_draft(
    state: State<'_, AppState>,
    profile_id: String,
    draft_json: String,
) -> IpcResult<()> {
    let pid = parse_profile_id(&profile_id)?;
    collections(&state).save_draft(&pid, &draft_json).into_ipc()
}

/// Publishes a collection recipe as its next, unchangeable revision.
#[tauri::command]
pub fn publish_collection_revision<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    recipe_json: String,
) -> IpcResult<manager_app::api::dto::CollectionRevisionDto> {
    events::after_state_change(&app, || {
        collections(&state).publish(&recipe_json).into_ipc()
    })
}

#[tauri::command]
pub fn list_collection_revisions(
    state: State<'_, AppState>,
    collection_id: String,
) -> IpcResult<Vec<manager_app::api::dto::CollectionRevisionDto>> {
    collections(&state).revisions(&collection_id).into_ipc()
}
