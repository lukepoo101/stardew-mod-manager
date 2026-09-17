//! The installation inspector adapters.
//!
//! One inspector knows every platform layout the manager understands, and the
//! request carries the operating system to interpret. That is deliberate: the
//! manager has to be able to explain a Windows installation on a Linux host
//! (a Proton prefix, a copied folder) and vice versa, and the layout is data
//! rather than a compile-time choice.

use crate::platform::shared::inspector::inspect_with_layout;
use crate::platform::shared::layouts::layout_for;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::discovery::GameInstallationInspectorPort;
use manager_core::game::{GameInspection, OperatingSystem, Storefront};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct PlatformGameInspector {
    operating_system: OperatingSystem,
}

impl PlatformGameInspector {
    pub fn new(operating_system: OperatingSystem) -> Self {
        Self { operating_system }
    }

    pub fn operating_system(&self) -> OperatingSystem {
        self.operating_system
    }
}

impl GameInstallationInspectorPort for PlatformGameInspector {
    fn inspect(
        &self,
        path: &Path,
        storefront: Storefront,
        operating_system: OperatingSystem,
    ) -> AppResult<GameInspection> {
        let Some(layout) = layout_for(operating_system) else {
            return Err(AppError::validation(
                "UNSUPPORTED_PLATFORM",
                format!(
                    "Game installation inspection is not implemented for {}",
                    operating_system.as_key()
                ),
            ));
        };
        inspect_with_layout(path, storefront, layout)
    }
}
