//! The shared launch-specification builder.
//!
//! Building a launch is the same decision on every platform - which launcher,
//! which working directory, which mod isolation argument - and only the file
//! names and the isolation flag differ. The platform-specific part is the
//! layout it is given.

use crate::platform::shared::inspector::InstallationLayout;
use manager_app::error::{AppError, AppResult};
use manager_core::game::{GameInstallation, OperatingSystem};
use manager_core::launch::{LaunchMode, LaunchSpec};
use std::path::Path;

#[allow(clippy::result_large_err)]
pub fn build_launch_spec(
    layout: &InstallationLayout,
    game: &GameInstallation,
    mode: LaunchMode,
    mods_path: Option<&Path>,
) -> AppResult<LaunchSpec> {
    if game.operating_system != layout.operating_system {
        return Err(AppError::validation(
            "UNSUPPORTED_PLATFORM",
            format!(
                "This installation is registered as a {} installation, but the {} launch layout is in use",
                game.operating_system.as_key(),
                layout.operating_system.as_key()
            ),
        ));
    }

    let modded = matches!(mode, LaunchMode::Modded | LaunchMode::RuntimeTest);
    let mut args = Vec::new();
    let executable = if modded {
        if let Some(mods_path) = mods_path {
            args.push("--mods-path".to_string());
            args.push(mods_path.to_string_lossy().to_string());
        }
        game.canonical_root.join(layout.canonical_smapi_launcher)
    } else {
        game.canonical_root.join(layout.canonical_vanilla_launcher)
    };

    Ok(LaunchSpec {
        executable,
        args,
        working_dir: game.canonical_root.clone(),
        env: Vec::new(),
    })
}

/// Rejects a launch whose installation belongs to another operating system.
#[allow(clippy::result_large_err)]
pub fn require_host_operating_system(
    game: &GameInstallation,
    host: OperatingSystem,
) -> AppResult<()> {
    if game.operating_system == host {
        return Ok(());
    }
    Err(AppError::validation(
        "UNSUPPORTED_PLATFORM",
        format!(
            "Launching a {} installation from a {} build is not supported",
            game.operating_system.as_key(),
            host.as_key()
        ),
    ))
}
