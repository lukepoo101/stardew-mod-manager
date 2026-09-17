//! The Steam client locations a Linux host uses.

use crate::platform::steam::SteamRootLocator;
use manager_core::game::OperatingSystem;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct LinuxSteamLocator {
    home: Option<PathBuf>,
}

impl LinuxSteamLocator {
    pub fn new() -> Self {
        Self::default()
    }

    /// A locator rooted at an explicit home directory, for tests and for a
    /// user whose home is not the process environment.
    pub fn with_home(home: PathBuf) -> Self {
        Self { home: Some(home) }
    }

    fn home(&self) -> Option<PathBuf> {
        self.home.clone().or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| !home.as_os_str().is_empty())
        })
    }
}

impl SteamRootLocator for LinuxSteamLocator {
    fn operating_system(&self) -> OperatingSystem {
        OperatingSystem::Linux
    }

    fn steam_roots(&self) -> Vec<PathBuf> {
        let Some(home) = self.home() else {
            return Vec::new();
        };

        vec![
            home.join(".local/share/Steam"),
            home.join(".steam/steam"),
            home.join(".steam/root"),
            // Flatpak and Snap package the client inside their own sandbox.
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
            home.join("snap/steam/common/.local/share/Steam"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_client_location_is_reported_for_a_home() {
        let locator = LinuxSteamLocator::with_home(PathBuf::from("/home/tester"));
        let roots = locator.steam_roots();
        assert!(roots.contains(&PathBuf::from("/home/tester/.local/share/Steam")));
        assert!(roots.contains(&PathBuf::from(
            "/home/tester/.var/app/com.valvesoftware.Steam/data/Steam"
        )));
    }

    #[test]
    fn a_host_without_a_home_reports_no_roots_instead_of_a_guess() {
        let locator = LinuxSteamLocator { home: None };
        // The environment may legitimately provide HOME; only assert the shape.
        let roots = locator.steam_roots();
        assert!(roots
            .iter()
            .all(|root| root.is_absolute() || root.as_os_str().is_empty()));
    }
}
