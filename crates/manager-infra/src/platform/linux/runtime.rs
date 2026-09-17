//! The native Linux launch layout.

use crate::platform::shared::layouts::LINUX_LAYOUT;
use manager_app::error::AppResult;
use manager_app::ports::runtime_layout::GameRuntimePort;
use manager_core::game::{GameInstallation, OperatingSystem};
use manager_core::launch::{LaunchMode, LaunchSpec};
use std::path::Path;

#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxGameRuntime;

impl LinuxGameRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl GameRuntimePort for LinuxGameRuntime {
    fn operating_system(&self) -> OperatingSystem {
        OperatingSystem::Linux
    }

    fn build_launch_spec(
        &self,
        game: &GameInstallation,
        mode: LaunchMode,
        mods_path: Option<&Path>,
    ) -> AppResult<LaunchSpec> {
        crate::platform::shared::runtime::build_launch_spec(&LINUX_LAYOUT, game, mode, mods_path)
    }

    fn is_game_process_image(&self, image_file_name: &str) -> bool {
        crate::platform::linux::process::expected_process_images()
            .iter()
            .any(|expected| expected.eq_ignore_ascii_case(image_file_name))
    }
}
