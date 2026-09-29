//! The shared game-directory inspection algorithm.
//!
//! Everything that can be described without knowing the platform lives here:
//! version extraction from the game's dependency manifest, writability probing,
//! bundled-mod detection, existing-SMAPI detection and support classification.
//! A platform contributes only the layout it recognises and how it phrases the
//! rejection of another platform's layout.

use crate::platform::shared::deps;
use chrono::Utc;
use manager_app::error::AppResult;
use manager_core::game::{
    classify_game_support, GameInspection, OperatingSystem, Storefront, SupportState,
};

/// The files that identify a game installation on one platform.
#[derive(Debug, Clone, Copy)]
pub struct InstallationLayout {
    pub operating_system: OperatingSystem,
    /// Launcher file names accepted on this platform, in preference order.
    pub launcher_names: &'static [&'static str],
    /// Every file that must be present for the layout to be complete.
    pub required_files: &'static [&'static str],
    /// Prefixes of the SMAPI launcher artifact on this platform.
    pub smapi_launcher_names: &'static [&'static str],
    /// The exact launcher path the runtime will execute for a modded launch.
    pub canonical_smapi_launcher: &'static str,
    /// The exact launcher path the runtime will execute for a vanilla launch.
    pub canonical_vanilla_launcher: &'static str,
    /// How a complete installation of a different platform is reported.
    pub foreign_layout_evidence: &'static str,
    /// The other platform whose file set could also be present here.
    ///
    /// A Proton or Wine prefix on Linux, and a WSL or copied folder on Windows,
    /// both look like a complete installation of the other platform. Reporting
    /// that as "another platform" rather than as a broken folder is the
    /// difference between a user understanding the answer and not.
    pub foreign_operating_system: OperatingSystem,
}

/// Inspects a candidate directory using a platform layout.
#[allow(clippy::result_large_err)]
pub fn inspect_with_layout(
    path: &std::path::Path,
    storefront: Storefront,
    layout: &InstallationLayout,
) -> AppResult<GameInspection> {
    let canonical_root = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut evidence = Vec::new();

    if !canonical_root.is_dir() {
        return Ok(GameInspection {
            installation_id: None,
            canonical_root,
            operating_system: layout.operating_system,
            storefront,
            observed_game_version: None,
            observed_smapi_version: None,
            has_existing_smapi: false,
            has_existing_mods: false,
            is_writable: false,
            support_state: SupportState::InvalidGameDirectory,
            evidence: vec!["Directory does not exist or is not a directory".to_string()],
            inspected_at: Utc::now(),
        });
    }

    let launcher = layout
        .launcher_names
        .iter()
        .find(|name| canonical_root.join(name).is_file())
        .copied();
    if launcher.is_none() {
        evidence.push(format!(
            "None of the expected game launchers were found ({})",
            layout.launcher_names.join(", ")
        ));
    }

    let mut missing_required = Vec::new();
    for required in layout.required_files {
        if !canonical_root.join(required).is_file() {
            missing_required.push(*required);
        }
    }
    if !missing_required.is_empty() {
        evidence.push(format!(
            "Missing required game files: {}",
            missing_required.join(", ")
        ));
    }

    let has_launcher = launcher.is_some();
    let is_complete = has_launcher && missing_required.is_empty();

    if !is_complete && !has_launcher {
        evidence.push(layout.foreign_layout_evidence.to_string());
    }

    // Recognise the other platform's complete layout so the answer is "this is
    // a Windows installation" rather than "this folder is invalid".
    let foreign_operating_system = if is_complete {
        None
    } else {
        crate::platform::shared::layouts::layout_for(layout.foreign_operating_system)
            .filter(|foreign| installation_matches(foreign, &canonical_root))
            .map(|foreign| foreign.operating_system)
    };
    if let Some(foreign) = foreign_operating_system {
        evidence.push(format!(
            "This folder is a {} installation, which this build cannot manage",
            foreign.as_key()
        ));
    }

    // A unique temporary file never truncates a user's existing file and never
    // follows a probe symlink into somewhere else.
    let is_writable = tempfile::Builder::new()
        .prefix(".smm-probe-")
        .tempfile_in(&canonical_root)
        .is_ok();
    evidence.push(if is_writable {
        "Game directory is writable".to_string()
    } else {
        "Game directory is NOT writable".to_string()
    });

    let has_smapi = layout
        .smapi_launcher_names
        .iter()
        .any(|name| canonical_root.join(name).exists())
        || canonical_root.join("smapi-internal").is_dir()
        || canonical_root.join("StardewModdingAPI.dll").is_file();
    if has_smapi {
        evidence.push("Existing SMAPI installation detected".to_string());
    }

    let has_mods = detect_user_mods(&canonical_root, &mut evidence);
    let observed_game_version = deps::game_version(&canonical_root);
    let observed_smapi_version =
        crate::smapi_adapter::detect_installed_smapi_version(&canonical_root);

    // An installation of another platform is an unsupported platform hosting a
    // real installation, which is a different answer from an invalid folder -
    // and the reason a Proton prefix on Linux is not reported as "broken".
    let support_state = classify_game_support(
        is_complete || foreign_operating_system.is_some(),
        foreign_operating_system.is_none(),
        is_writable,
        has_smapi,
        has_mods,
        false,
    );

    Ok(GameInspection {
        installation_id: None,
        canonical_root,
        operating_system: layout.operating_system,
        storefront,
        observed_game_version,
        observed_smapi_version,
        has_existing_smapi: has_smapi,
        has_existing_mods: has_mods,
        is_writable,
        support_state,
        evidence,
        inspected_at: Utc::now(),
    })
}

/// Whether a directory presents a complete installation for a layout.
fn installation_matches(layout: &InstallationLayout, root: &std::path::Path) -> bool {
    layout
        .launcher_names
        .iter()
        .any(|name| root.join(name).is_file())
        && layout
            .required_files
            .iter()
            .all(|name| root.join(name).is_file())
}

/// Mods shipped with SMAPI, which do not represent user content.
pub const BUNDLED_MOD_FOLDERS: &[&str] = &["ConsoleCommands", "SaveBackup"];

fn detect_user_mods(root: &std::path::Path, evidence: &mut Vec<String>) -> bool {
    let mods_dir = root.join("Mods");
    if !mods_dir.is_dir() {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(&mods_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if BUNDLED_MOD_FOLDERS.contains(&name.as_str()) {
            continue;
        }
        evidence.push(format!("Found existing mod directory: {}", name));
        return true;
    }
    false
}
