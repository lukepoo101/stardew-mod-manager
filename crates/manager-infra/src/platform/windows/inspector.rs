//! Recognition of a native Windows Stardew Valley installation.

use crate::platform::shared::inspector::inspect_with_layout;
use crate::platform::shared::layouts::WINDOWS_LAYOUT;
use manager_app::error::AppResult;
use manager_app::ports::discovery::GameInstallationInspectorPort;
use manager_core::game::{GameInspection, OperatingSystem, Storefront};
use std::path::Path;

/// An inspector pinned to the Windows layout.
///
/// The application wires the general inspector; this type exists for callers
/// and tests that mean "this installation is Windows" explicitly, such as the
/// Steam discovery path on a Windows host.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsGameInspector;

impl WindowsGameInspector {
    pub fn new() -> Self {
        Self
    }

    /// Inspects a path without going through the port object.
    #[allow(clippy::result_large_err)]
    pub fn inspect_path(path: &Path, storefront: Storefront) -> AppResult<GameInspection> {
        inspect_with_layout(path, storefront, &WINDOWS_LAYOUT)
    }
}

impl GameInstallationInspectorPort for WindowsGameInspector {
    fn inspect(
        &self,
        path: &Path,
        storefront: Storefront,
        _operating_system: OperatingSystem,
    ) -> AppResult<GameInspection> {
        inspect_with_layout(path, storefront, &WINDOWS_LAYOUT)
    }
}
