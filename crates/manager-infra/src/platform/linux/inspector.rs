//! Recognition of a native Linux Stardew Valley installation.

use crate::platform::shared::inspector::inspect_with_layout;
use crate::platform::shared::layouts::LINUX_LAYOUT;
use manager_app::error::AppResult;
use manager_app::ports::discovery::GameInstallationInspectorPort;
use manager_core::game::{GameInspection, OperatingSystem, Storefront};
use std::path::Path;

/// An inspector pinned to the Linux layout.
///
/// The application wires the general inspector; this type exists for callers
/// and tests that mean "this installation is Linux" explicitly.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxGameInspector;

impl LinuxGameInspector {
    pub fn new() -> Self {
        Self
    }

    /// Inspects a path without going through the port object.
    #[allow(clippy::result_large_err)]
    pub fn inspect_path(path: &Path, storefront: Storefront) -> AppResult<GameInspection> {
        inspect_with_layout(path, storefront, &LINUX_LAYOUT)
    }
}

impl GameInstallationInspectorPort for LinuxGameInspector {
    fn inspect(
        &self,
        path: &Path,
        storefront: Storefront,
        _operating_system: OperatingSystem,
    ) -> AppResult<GameInspection> {
        inspect_with_layout(path, storefront, &LINUX_LAYOUT)
    }
}
