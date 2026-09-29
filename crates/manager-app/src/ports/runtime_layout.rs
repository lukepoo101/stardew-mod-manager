use crate::error::AppResult;
use manager_core::game::{GameInstallation, OperatingSystem};
use manager_core::launch::{LaunchMode, LaunchSpec};
use std::path::Path;

/// Builds the platform-specific launch specification for one game installation.
///
/// Executable names, extension conventions, working directory requirements and
/// profile isolation arguments all differ per platform, and the application
/// layer must not know any of them. It asks for a launch specification and
/// hands that to the process backend.
pub trait GameRuntimePort: Send + Sync {
    /// The operating system whose layout this runtime describes.
    fn operating_system(&self) -> OperatingSystem;

    /// Whether this runtime can launch installations for the given platform.
    ///
    /// A Windows runtime cannot start a Linux installation, and launching a
    /// Proton or Wine prefix through the manager is not supported.
    fn supports(&self, operating_system: OperatingSystem) -> bool {
        self.operating_system() == operating_system
    }

    /// The launch specification for one game installation.
    ///
    /// The mods path is the profile mods directory the launch must advertise
    /// through the platform's mod isolation mechanism.
    fn build_launch_spec(
        &self,
        game: &GameInstallation,
        mode: LaunchMode,
        mods_path: Option<&Path>,
    ) -> AppResult<LaunchSpec>;

    /// Whether a running process image is this platform's game or SMAPI.
    ///
    /// Process backends use this to recognise installs that Steam, SMAPI or an
    /// earlier manager session started.
    fn is_game_process_image(&self, image_file_name: &str) -> bool;
}
