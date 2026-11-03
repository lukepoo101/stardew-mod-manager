//! The shared launch-specification builder.
//!
//! Building a launch is the same decision on every platform - which launcher,
//! which working directory, which mod isolation argument - and only the file
//! names and the isolation flag differ. The platform-specific part is the
//! layout it is given.

use crate::platform::shared::inspector::InstallationLayout;
use manager_app::error::{AppError, AppResult};
use manager_core::game::{GameInstallation, OperatingSystem, Storefront};
use manager_core::launch::{LaunchMode, LaunchSpec};
use std::path::{Path, PathBuf};

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
        vanilla_launcher(layout, &game.canonical_root, game.storefront)?
    };

    Ok(LaunchSpec {
        executable,
        args,
        working_dir: game.canonical_root.clone(),
        env: storefront_env(game.storefront),
    })
}

/// Whether a launcher file is SMAPI's launcher script rather than the game's.
///
/// SMAPI's Unix launcher is a shell script that starts StardewModdingAPI;
/// the game's own launchers never mention it.
fn is_smapi_launcher(path: &Path) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    if file.take(64 * 1024).read_to_end(&mut head).is_err() {
        return false;
    }
    head.windows(b"StardewModdingAPI".len())
        .any(|window| window == b"StardewModdingAPI")
}

/// The launcher that starts the unmodified game, read from the folder.
///
/// Where SMAPI's installer replaces the game's launcher (Linux and macOS),
/// the game's own launcher is where SMAPI moved it. That copy is used only
/// when it really is the game's: the installer moves whatever is in the
/// launcher's place into an empty backup slot, so a reinstall after the
/// backup was lost puts SMAPI's own script there. Otherwise the canonical
/// launcher, then any other launcher of the game that is not SMAPI's, is
/// used; when there is none, starting "vanilla" would start SMAPI, so it is
/// refused.
#[allow(clippy::result_large_err)]
pub fn vanilla_launcher(
    layout: &InstallationLayout,
    root: &Path,
    storefront: Storefront,
) -> AppResult<PathBuf> {
    if let Some(moved) = layout.smapi_moved_vanilla_launcher {
        let original = root.join(moved);
        if original.is_file() && !is_smapi_launcher(&original) {
            return Ok(original);
        }
    }
    let canonical = root.join(layout.canonical_vanilla_launcher);
    // A missing launcher is not SMAPI's either; the launch checks report it.
    if !is_smapi_launcher(&canonical) {
        return Ok(canonical);
    }
    if let Some(other) = layout
        .launcher_names
        .iter()
        .map(|name| root.join(name))
        .find(|candidate| candidate.is_file() && !is_smapi_launcher(candidate))
    {
        return Ok(other);
    }
    // Reinstalling SMAPI cannot bring the launcher back: with the backup
    // slot empty, its installer moves its own script there.
    let advice = match storefront {
        Storefront::Steam => {
            "Verify the game files in Steam (the game's Properties, Installed Files, Verify integrity of game files) to get it back."
        }
        Storefront::Gog => "Repair or reinstall the game from GOG to get it back.",
        _ => "Reinstall the game to get it back.",
    };
    Err(AppError::validation(
        "VANILLA_LAUNCHER_REPLACED",
        format!(
            "{} in the game folder now starts SMAPI, and the game's own launcher{} is not there, so the unmodified game cannot be started from here. {advice}",
            layout.canonical_vanilla_launcher,
            layout
                .smapi_moved_vanilla_launcher
                .map(|moved| format!(" ({moved})"))
                .unwrap_or_default()
        ),
    ))
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
mod vanilla_launcher_tests {
    use super::*;
    use crate::platform::shared::layouts::{LINUX_LAYOUT, WINDOWS_LAYOUT};
    use chrono::Utc;
    use manager_core::game::{ManagementMode, Storefront};
    use manager_core::ids::GameInstallationId;

    /// SMAPI's unix-launcher.sh, as its installer copies it to StardewValley.
    const SMAPI_SCRIPT: &str =
        "#!/usr/bin/env bash\ncd \"$(dirname \"$0\")\" || exit $?\nLAUNCH_FILE=\"./StardewModdingAPI\"\nexec $LAUNCH_FILE \"$@\"\n";
    const GAME_SCRIPT: &str = "#!/bin/sh\nexec ./\"Stardew Valley\" \"$@\"\n";

    fn game(root: &Path, operating_system: OperatingSystem) -> GameInstallation {
        GameInstallation {
            id: GameInstallationId::new(),
            canonical_root: root.to_path_buf(),
            operating_system,
            storefront: Storefront::Steam,
            management_mode: ManagementMode::Managed,
            created_at: Utc::now(),
        }
    }

    fn write(root: &Path, name: &str, content: &str) {
        std::fs::write(root.join(name), content).unwrap();
    }

    #[allow(clippy::result_large_err)]
    fn vanilla(root: &Path, os: OperatingSystem) -> AppResult<LaunchSpec> {
        let layout = if os == OperatingSystem::Windows {
            &WINDOWS_LAYOUT
        } else {
            &LINUX_LAYOUT
        };
        build_launch_spec(layout, os, &game(root, os), LaunchMode::Vanilla, None)
    }

    #[test]
    fn linux_without_smapi_starts_the_games_launcher() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "StardewValley", GAME_SCRIPT);
        let spec = vanilla(tmp.path(), OperatingSystem::Linux).unwrap();
        assert_eq!(spec.executable, tmp.path().join("StardewValley"));
        assert!(spec.args.is_empty());
    }

    #[test]
    fn linux_with_smapi_starts_the_launcher_smapi_moved_aside() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "StardewValley", SMAPI_SCRIPT);
        write(tmp.path(), "StardewValley-original", GAME_SCRIPT);
        write(tmp.path(), "StardewModdingAPI", "elf");
        let spec = vanilla(tmp.path(), OperatingSystem::Linux).unwrap();
        assert_eq!(spec.executable, tmp.path().join("StardewValley-original"));
        assert!(spec.args.is_empty(), "vanilla never passes --mods-path");

        // The modded launch is unchanged.
        let modded = build_launch_spec(
            &LINUX_LAYOUT,
            OperatingSystem::Linux,
            &game(tmp.path(), OperatingSystem::Linux),
            LaunchMode::Modded,
            Some(Path::new("/profile/Mods")),
        )
        .unwrap();
        assert_eq!(modded.executable, tmp.path().join("StardewModdingAPI"));
    }

    #[test]
    fn linux_with_smapi_and_no_moved_launcher_uses_the_game_binary() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "StardewValley", SMAPI_SCRIPT);
        write(tmp.path(), "Stardew Valley", "native apphost");
        let spec = vanilla(tmp.path(), OperatingSystem::Linux).unwrap();
        assert_eq!(spec.executable, tmp.path().join("Stardew Valley"));
    }

    #[test]
    fn linux_with_only_smapis_launcher_refuses_rather_than_starting_smapi() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "StardewValley", SMAPI_SCRIPT);
        let error = vanilla(tmp.path(), OperatingSystem::Linux).unwrap_err();
        assert_eq!(error.code, "VANILLA_LAUNCHER_REPLACED");
        assert!(error.to_string().contains("StardewValley-original"));
        assert!(error.to_string().contains("Verify the game files in Steam"));
        assert!(!error.to_string().contains("Reinstalling SMAPI"));
    }

    /// Review S2: after a reinstall over a lost backup, SMAPI's installer has
    /// moved its own script into the backup slot. That must not be started.
    #[test]
    fn a_backup_slot_holding_smapis_script_is_not_the_game() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "StardewValley", SMAPI_SCRIPT);
        write(tmp.path(), "StardewValley-original", SMAPI_SCRIPT);
        write(tmp.path(), "Stardew Valley", "native apphost");
        let spec = vanilla(tmp.path(), OperatingSystem::Linux).unwrap();
        assert_eq!(spec.executable, tmp.path().join("Stardew Valley"));

        std::fs::remove_file(tmp.path().join("Stardew Valley")).unwrap();
        assert_eq!(
            vanilla(tmp.path(), OperatingSystem::Linux)
                .unwrap_err()
                .code,
            "VANILLA_LAUNCHER_REPLACED"
        );
    }

    #[test]
    fn windows_with_smapi_starts_the_games_exe() {
        // SMAPI on Windows adds StardewModdingAPI.exe and renames nothing.
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "Stardew Valley.exe", "MZ game");
        write(tmp.path(), "StardewModdingAPI.exe", "MZ StardewModdingAPI");
        let spec = vanilla(tmp.path(), OperatingSystem::Windows).unwrap();
        assert_eq!(spec.executable, tmp.path().join("Stardew Valley.exe"));
        assert!(spec.args.is_empty());
    }
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
