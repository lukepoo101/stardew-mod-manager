//! Steam discovery as a composition-port implementation.
//!
//! The locator is injected, so the same object serves Linux, Windows and any
//! future platform with a different set of client locations.

#[cfg(not(target_os = "windows"))]
use crate::platform::linux::steam::LinuxSteamLocator;
use crate::platform::steam::{self, SteamRootLocator};
#[cfg(target_os = "windows")]
use crate::platform::windows::steam::WindowsSteamLocator;
use manager_app::ports::discovery::{GameCandidate, GameDiscoveryPort};
use manager_core::game::OperatingSystem;
use std::path::PathBuf;

/// Steam discovery for one operating system.
pub struct SteamGameDiscovery {
    locator: Box<dyn SteamRootLocator>,
}

impl SteamGameDiscovery {
    pub fn new(locator: Box<dyn SteamRootLocator>) -> Self {
        Self { locator }
    }

    /// Discovery using the Linux client locations.
    #[cfg(not(target_os = "windows"))]
    pub fn linux() -> Self {
        Self::new(Box::new(LinuxSteamLocator::new()))
    }

    /// Discovery using the Windows client locations.
    #[cfg(target_os = "windows")]
    pub fn windows() -> Self {
        Self::new(Box::new(WindowsSteamLocator::new()))
    }

    /// Discovery using the client locations of the running host.
    #[cfg(target_os = "windows")]
    pub fn for_host() -> Self {
        Self::windows()
    }

    /// Discovery using the client locations of the running host.
    #[cfg(not(target_os = "windows"))]
    pub fn for_host() -> Self {
        Self::linux()
    }

    /// Installations reachable from explicit roots.
    ///
    /// This is the seam the platform tests use: the roots are supplied so the
    /// discovery and validation logic can be exercised without a real Steam
    /// installation.
    pub fn discover_installations_from_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
        steam::discover_installations_from_roots(roots, OperatingSystem::host())
    }

    /// Canonical roots of the installations found by the injected locator.
    pub fn discover_installations(&self) -> Vec<PathBuf> {
        steam::discover_with_locator(self.locator.as_ref())
            .into_iter()
            .map(|candidate| candidate.path)
            .collect()
    }
}

impl GameDiscoveryPort for SteamGameDiscovery {
    fn operating_system(&self) -> OperatingSystem {
        self.locator.operating_system()
    }

    fn discover(&self) -> Vec<GameCandidate> {
        steam::discover_with_locator(self.locator.as_ref())
    }

    fn describe_searched_locations(&self) -> Vec<PathBuf> {
        self.locator.steam_roots()
    }
}
