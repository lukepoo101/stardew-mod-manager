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

/// Builds a launch specification for a game installation.
///
/// The platform is the one the caller is launching on, which is not always the
/// platform the layout declares: the POSIX layout is shared by Linux and macOS,
/// so the game's recorded operating system has to be compared with the caller's
/// rather than with an internal field.
#[allow(clippy::result_large_err)]
pub fn build_launch_spec(
    layout: &InstallationLayout,
    platform: OperatingSystem,
    game: &GameInstallation,
    mode: LaunchMode,
    mods_path: Option<&Path>,
) -> AppResult<LaunchSpec> {
    if game.operating_system != platform {
        return Err(AppError::validation(
            "UNSUPPORTED_PLATFORM",
            format!(
                "This installation is registered as a {} installation, but this build launches {} installations",
                game.operating_system.as_key(),
                platform.as_key()
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
        env: storefront_env(game.storefront),
    })
}

/// Stardew Valley's Steam app id.
pub const STEAM_APP_ID: &str = "413150";

/// What a storefront needs in the environment when the game is started
/// directly rather than through it. Steam's documented app id variables let
/// Steamworks start without Steam relaunching the game (which would drop
/// the profile's mods path). Other storefronts need nothing.
pub fn storefront_env(storefront: manager_core::game::Storefront) -> Vec<(String, String)> {
    match storefront {
        manager_core::game::Storefront::Steam => vec![
            ("SteamAppId".to_string(), STEAM_APP_ID.to_string()),
            ("SteamGameId".to_string(), STEAM_APP_ID.to_string()),
        ],
        _ => Vec::new(),
    }
}

/// What a launch spec needs that is not on disk.
pub fn missing_launch_files(spec: &manager_core::launch::LaunchSpec) -> Vec<String> {
    let mut missing = Vec::new();
    if !spec.working_dir.is_dir() {
        missing.push(format!("the game folder {}", spec.working_dir.display()));
    }
    if !spec.executable.is_file() {
        missing.push(format!("the launcher {}", spec.executable.display()));
    }
    missing
}

#[cfg(test)]
mod storefront_tests {
    use super::*;
    use manager_core::game::Storefront;

    #[test]
    fn only_steam_installs_get_the_app_id() {
        assert!(storefront_env(Storefront::Steam)
            .iter()
            .any(|(k, v)| k == "SteamAppId" && v == STEAM_APP_ID));
        for other in [Storefront::Gog, Storefront::Manual, Storefront::Unknown] {
            assert!(storefront_env(other).is_empty());
        }
    }
}
