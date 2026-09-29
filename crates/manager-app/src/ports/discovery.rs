use crate::error::AppResult;
use manager_core::game::{GameInspection, OperatingSystem, Storefront};
use std::path::{Path, PathBuf};

/// A directory that may hold a Stardew Valley installation.
///
/// The candidate carries the platform it was discovered for, so a candidate
/// that Steam produced inside a Proton prefix is never mistaken for a native
/// installation of the host platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameCandidate {
    pub path: PathBuf,
    pub storefront: Storefront,
    pub operating_system: OperatingSystem,
}

pub trait GameDiscoveryPort: Send + Sync {
    /// The operating system whose installations this discovery reports.
    fn operating_system(&self) -> OperatingSystem;

    fn discover(&self) -> Vec<GameCandidate>;

    /// The locations this discovery inspects, for diagnostics.
    ///
    /// Reported whether or not they exist, so a user can see where the manager
    /// looked rather than only that it found nothing.
    fn describe_searched_locations(&self) -> Vec<std::path::PathBuf> {
        Vec::new()
    }
}

pub trait GameInstallationInspectorPort: Send + Sync {
    /// Inspects a candidate directory as an installation of the given platform.
    ///
    /// The platform is a request parameter because the manager must be able to
    /// describe an installation that does not belong to the running host.
    fn inspect(
        &self,
        path: &Path,
        storefront: Storefront,
        operating_system: OperatingSystem,
    ) -> AppResult<GameInspection>;
}
