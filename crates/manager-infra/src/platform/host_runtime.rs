//! Launch layouts as adapters.
//!
//! A layout is data (file names and an isolation flag), so this module has two
//! jobs: select the layout of the running host, and provide a layout for a named
//! platform so a test - or a future "show me where this would launch" surface -
//! can build a specification for an installation that is not this host's.

use crate::platform::shared::layouts::layout_for;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::runtime_layout::GameRuntimePort;
use manager_core::game::{GameInstallation, OperatingSystem};
use manager_core::launch::{LaunchMode, LaunchSpec};
use std::path::Path;

/// The launch layout of the running host.
///
/// This is what the application wires up, and it refuses to build a launch
/// specification for an installation that belongs to another platform: a
/// Windows install cannot be started by a Linux build, and saying so here keeps
/// the failure before a process is spawned rather than after.
#[derive(Debug, Clone, Copy)]
pub struct HostGameRuntime {
    operating_system: OperatingSystem,
}

impl HostGameRuntime {
    pub fn new() -> Self {
        Self {
            operating_system: OperatingSystem::host(),
        }
    }
}

impl Default for HostGameRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl GameRuntimePort for HostGameRuntime {
    fn operating_system(&self) -> OperatingSystem {
        self.operating_system
    }

    fn build_launch_spec(
        &self,
        game: &GameInstallation,
        mode: LaunchMode,
        mods_path: Option<&Path>,
    ) -> AppResult<LaunchSpec> {
        if game.operating_system != self.operating_system {
            return Err(AppError::validation(
                "UNSUPPORTED_PLATFORM",
                format!(
                    "This installation is registered as a {} installation, but this build can only launch {} installations",
                    game.operating_system.as_key(),
                    self.operating_system.as_key()
                ),
            ));
        }
        let layout = layout_for(self.operating_system).ok_or_else(|| {
            AppError::validation(
                "UNSUPPORTED_PLATFORM",
                format!(
                    "Launching is not implemented for {}",
                    self.operating_system.as_key()
                ),
            )
        })?;
        crate::platform::shared::runtime::build_launch_spec(layout, game, mode, mods_path)
    }

    fn is_game_process_image(&self, image_file_name: &str) -> bool {
        process_image_matches(self.operating_system, image_file_name)
    }
}

/// A launch layout for a named platform, without the host check.
///
/// Integration tests use this because they construct synthetic installations
/// for an arbitrary platform and run on whatever host CI provides.
#[derive(Debug, Clone, Copy)]
pub struct TestGameRuntime {
    operating_system: OperatingSystem,
}

impl TestGameRuntime {
    pub fn for_platform(operating_system: OperatingSystem) -> Self {
        Self { operating_system }
    }
}

impl GameRuntimePort for TestGameRuntime {
    fn operating_system(&self) -> OperatingSystem {
        self.operating_system
    }

    fn build_launch_spec(
        &self,
        game: &GameInstallation,
        mode: LaunchMode,
        mods_path: Option<&Path>,
    ) -> AppResult<LaunchSpec> {
        let layout = layout_for(self.operating_system).ok_or_else(|| {
            AppError::validation(
                "UNSUPPORTED_PLATFORM",
                format!(
                    "Launching is not implemented for {}",
                    self.operating_system.as_key()
                ),
            )
        })?;
        crate::platform::shared::runtime::build_launch_spec(layout, game, mode, mods_path)
    }

    fn is_game_process_image(&self, image_file_name: &str) -> bool {
        process_image_matches(self.operating_system, image_file_name)
    }
}

fn process_image_matches(operating_system: OperatingSystem, image_file_name: &str) -> bool {
    let Some(layout) = layout_for(operating_system) else {
        return false;
    };
    layout
        .launcher_names
        .iter()
        .chain(layout.smapi_launcher_names.iter())
        .any(|expected| expected.eq_ignore_ascii_case(image_file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use manager_core::game::{ManagementMode, Storefront};
    use manager_core::ids::GameInstallationId;

    fn installation(operating_system: OperatingSystem) -> GameInstallation {
        GameInstallation {
            id: GameInstallationId::new(),
            canonical_root: std::path::PathBuf::from("/games/Stardew Valley"),
            operating_system,
            storefront: Storefront::Steam,
            management_mode: ManagementMode::Managed,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn the_host_runtime_refuses_another_platforms_installation() {
        let runtime = HostGameRuntime::new();
        let other = if cfg!(target_os = "windows") {
            OperatingSystem::Linux
        } else {
            OperatingSystem::Windows
        };
        let error = runtime
            .build_launch_spec(
                &installation(other),
                LaunchMode::Modded,
                Some(Path::new("/mods")),
            )
            .unwrap_err();
        assert_eq!(error.code, "UNSUPPORTED_PLATFORM");
    }

    #[test]
    fn the_host_runtime_builds_a_launch_for_its_own_platform() {
        let runtime = HostGameRuntime::new();
        let spec = runtime
            .build_launch_spec(
                &installation(runtime.operating_system()),
                LaunchMode::Modded,
                Some(Path::new("/mods")),
            )
            .unwrap();
        assert!(spec.executable.to_string_lossy().contains("Stardew"));
        // The mod isolation argument is the platform layout's decision, and
        // every layout the manager ships isolates profile mods.
        assert_eq!(spec.args.first().map(String::as_str), Some("--mods-path"));
        assert_eq!(spec.args.get(1).map(String::as_str), Some("/mods"));
    }

    #[test]
    fn the_host_runtime_matches_game_images_by_platform() {
        let runtime = HostGameRuntime::new();
        let name = if cfg!(target_os = "windows") {
            "StardewModdingAPI.exe"
        } else {
            "StardewModdingAPI"
        };
        assert!(runtime.is_game_process_image(name));
    }

    #[test]
    fn a_windows_launch_uses_the_exe_launchers() {
        let runtime = TestGameRuntime::for_platform(OperatingSystem::Windows);
        let spec = runtime
            .build_launch_spec(
                &installation(OperatingSystem::Windows),
                LaunchMode::Modded,
                Some(Path::new("C:/mods")),
            )
            .unwrap();
        assert!(spec
            .executable
            .to_string_lossy()
            .ends_with("StardewModdingAPI.exe"));

        let vanilla = runtime
            .build_launch_spec(
                &installation(OperatingSystem::Windows),
                LaunchMode::Vanilla,
                None,
            )
            .unwrap();
        assert!(vanilla
            .executable
            .to_string_lossy()
            .ends_with("Stardew Valley.exe"));
        assert!(vanilla.args.is_empty());
    }

    #[test]
    fn a_linux_launch_uses_the_extension_less_launchers() {
        let runtime = TestGameRuntime::for_platform(OperatingSystem::Linux);
        let spec = runtime
            .build_launch_spec(
                &installation(OperatingSystem::Linux),
                LaunchMode::Modded,
                Some(Path::new("/mods")),
            )
            .unwrap();
        assert!(spec
            .executable
            .to_string_lossy()
            .ends_with("StardewModdingAPI"));
    }

    #[test]
    fn process_images_are_recognised_per_platform() {
        let windows = TestGameRuntime::for_platform(OperatingSystem::Windows);
        assert!(windows.is_game_process_image("StardewModdingAPI.exe"));
        assert!(windows.is_game_process_image("stardew valley.EXE"));
        assert!(!windows.is_game_process_image("StardewModdingAPI"));

        let linux = TestGameRuntime::for_platform(OperatingSystem::Linux);
        assert!(linux.is_game_process_image("StardewValley"));
        assert!(linux.is_game_process_image("StardewModdingAPI"));
    }
}
