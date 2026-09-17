//! The POSIX launch layout.

use crate::platform::shared::layouts::LINUX_LAYOUT;
use manager_app::error::AppResult;
use manager_app::ports::runtime_layout::GameRuntimePort;
use manager_core::game::{GameInstallation, OperatingSystem};
use manager_core::launch::{LaunchMode, LaunchSpec};
use std::path::Path;

/// Launches Stardew Valley through SMAPI on Linux and macOS.
#[derive(Debug, Clone, Copy)]
pub struct PosixGameRuntime {
    operating_system: OperatingSystem,
}

impl PosixGameRuntime {
    pub fn new(operating_system: OperatingSystem) -> Self {
        Self { operating_system }
    }
}

impl GameRuntimePort for PosixGameRuntime {
    fn operating_system(&self) -> OperatingSystem {
        self.operating_system
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
        crate::platform::posix::process::expected_process_images()
            .iter()
            .any(|expected| expected.eq_ignore_ascii_case(image_file_name))
    }
}
