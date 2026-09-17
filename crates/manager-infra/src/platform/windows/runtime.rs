//! The native Windows launch layout.

use crate::platform::shared::layouts::WINDOWS_LAYOUT;
use manager_app::error::AppResult;
use manager_app::ports::runtime_layout::GameRuntimePort;
use manager_core::game::{GameInstallation, OperatingSystem};
use manager_core::launch::{LaunchMode, LaunchSpec};
use std::path::Path;

/// Launches Stardew Valley through SMAPI on Windows.
///
/// The manager starts StardewModdingAPI.exe directly with --mods-path, which is
/// what makes per-profile mod isolation work. Launching through Steam instead
/// would give the user the Steam overlay and playtime tracking but would not
/// accept a profile-specific mods path, so storefront-integrated launch is a
/// deliberate non-goal for this release.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsGameRuntime;

impl WindowsGameRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl GameRuntimePort for WindowsGameRuntime {
    fn operating_system(&self) -> OperatingSystem {
        OperatingSystem::Windows
    }

    fn build_launch_spec(
        &self,
        game: &GameInstallation,
        mode: LaunchMode,
        mods_path: Option<&Path>,
    ) -> AppResult<LaunchSpec> {
        crate::platform::shared::runtime::build_launch_spec(&WINDOWS_LAYOUT, game, mode, mods_path)
    }

    fn is_game_process_image(&self, image_file_name: &str) -> bool {
        crate::platform::windows::process::is_game_process_image(image_file_name)
    }
}
