use crate::state::AppState;
use manager_core::domain::*;
use manager_core::install::ArchiveInspectionResult;
use manager_core::ports::StateRepository;
use manager_core::use_cases::AppSnapshot;
use manager_infra::archive::SafeZipExtractor;
use manager_infra::discovery::SteamGameDiscovery;
use std::path::{Path, PathBuf};
use tauri::State;

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
pub fn prepare_smapi() -> Result<SmapiReleaseInfo, String> {
    Ok(manager_core::smapi::get_pinned_smapi_release())
}

#[tauri::command]
pub fn install_smapi(
    state: State<'_, AppState>,
    game_id: String,
) -> Result<SmapiInstallationRecord, String> {
    state.use_cases.install_smapi(&game_id, None)
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

    // Strip surrounding quotes
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        trimmed = trimmed[1..trimmed.len() - 1].trim();
    }

    // Strip file:// prefix
    let mut unescaped = if let Some(stripped) = trimmed.strip_prefix("file://") {
        stripped.to_string()
    } else {
        trimmed.to_string()
    };

    // Percent-decode basic URL encodings like %20
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

    // Check directly in ~/Downloads
    let in_downloads = Path::new(&home).join("Downloads").join(candidate);
    if in_downloads.exists() {
        return Ok(in_downloads);
    }

    // Extract filename and check ~/Downloads
    let filename = Path::new(candidate)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| candidate.to_string());

    let in_downloads_by_name = Path::new(&home).join("Downloads").join(&filename);
    if in_downloads_by_name.exists() {
        return Ok(in_downloads_by_name);
    }

    // Check Desktop
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

#[tauri::command]
pub fn pick_mod_file() -> Result<Option<String>, String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let downloads = format!("{}/Downloads", home);

    let output = std::process::Command::new("zenity")
        .args([
            "--file-selection",
            "--title=Select Stardew Valley Mod ZIP",
            "--file-filter=ZIP Archives (*.zip) | *.zip",
            "--filename",
            &format!("{}/", downloads),
        ])
        .output();

    if let Ok(out) = output {
        if out.status.success() {
            let path_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path_str.is_empty() && Path::new(&path_str).exists() {
                return Ok(Some(path_str));
            }
        }
    }

    Ok(None)
}

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
pub fn launch_game(
    state: State<'_, AppState>,
    game_id: String,
    setup_id: String,
) -> Result<LaunchSession, String> {
    if state.validate_setup(&setup_id)?.game_id != game_id {
        return Err("Setup does not belong to game".into());
    }
    let mods_dir = state.paths.mods_dir(&setup_id);
    state.use_cases.launch_game(&game_id, &setup_id, &mods_dir)
}

#[tauri::command]
pub fn get_operation(state: State<'_, AppState>, id: String) -> Result<Option<Operation>, String> {
    state.use_cases.repo.get_operation(&id)
}

#[tauri::command]
pub fn cancel_operation(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let _ = (state, id);
    Err("Started operations cannot be cancelled; use recovery to reconcile them".into())
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
) -> Result<Option<LaunchSession>, String> {
    state.use_cases.poll_session(&session_id)
}

#[tauri::command]
pub fn terminate_game(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<(), String> {
    state.use_cases.terminate_game(session_id.as_deref())
}

#[tauri::command]
pub fn get_smapi_log(state: State<'_, AppState>) -> Result<String, String> {
    state.use_cases.get_smapi_log()
}

#[tauri::command]
pub fn get_smapi_log_path(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state
        .use_cases
        .get_smapi_log_path()
        .to_string_lossy()
        .to_string())
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

        // Test 1: exact path
        let resolved =
            resolve_mod_file_path_in_home(test_file.to_str().unwrap(), temporary_home.path())
                .unwrap();
        assert_eq!(resolved, test_file);

        // Test 2: filename only
        let resolved2 =
            resolve_mod_file_path_in_home("test_mod_sample.zip", temporary_home.path()).unwrap();
        assert_eq!(resolved2, test_file);

        // Test 3: ~/Downloads/ filename
        let resolved3 =
            resolve_mod_file_path_in_home("~/Downloads/test_mod_sample.zip", temporary_home.path())
                .unwrap();
        assert_eq!(resolved3, test_file);

        // Test 4: file:// URI
        let uri = format!("file://{}", test_file.to_str().unwrap());
        let resolved4 = resolve_mod_file_path_in_home(&uri, temporary_home.path()).unwrap();
        assert_eq!(resolved4, test_file);

        // Test 5: quoted string
        let quoted = format!("\"{}\"", test_file.to_str().unwrap());
        let resolved5 = resolve_mod_file_path_in_home(&quoted, temporary_home.path()).unwrap();
        assert_eq!(resolved5, test_file);

        // Clean up
        let _ = std::fs::remove_file(test_file);
    }
}

#[tauri::command]
pub fn retry_recovery(state: State<'_, AppState>) -> Result<(), String> {
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
pub fn cancel_inspection(state: State<'_, AppState>, plan_id: String) -> Result<(), String> {
    state.pending_plans.remove(&plan_id);
    Ok(())
}
