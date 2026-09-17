//! The Steam client locations a POSIX host uses.

use crate::platform::steam::SteamRootLocator;
use manager_core::game::OperatingSystem;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct PosixSteamLocator {
    operating_system: OperatingSystem,
    home: Option<PathBuf>,
}

impl PosixSteamLocator {
    pub fn new(operating_system: OperatingSystem) -> Self {
        Self {
            operating_system,
            home: None,
        }
    }

    /// A locator rooted at an explicit home directory, for tests and for a
    /// user whose home is not the process environment.
    pub fn with_home(operating_system: OperatingSystem, home: PathBuf) -> Self {
        Self {
            operating_system,
            home: Some(home),
        }
    }

    fn home(&self) -> Option<PathBuf> {
        self.home.clone().or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|home| !home.as_os_str().is_empty())
        })
    }
}

impl SteamRootLocator for PosixSteamLocator {
    fn operating_system(&self) -> OperatingSystem {
        self.operating_system
    }

    fn steam_roots(&self) -> Vec<PathBuf> {
        let Some(home) = self.home() else {
            return Vec::new();
        };

        let mut roots = vec![
            // The client's own directories, which both platforms use.
            home.join(".local/share/Steam"),
            home.join(".steam/steam"),
            home.join(".steam/root"),
        ];
        if self.operating_system != OperatingSystem::MacOS {
            // Sandboxed Linux packages keep the client inside their own prefix.
            roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
            roots.push(home.join(".var/app/com.valvesoftware.Steam/data/Steam"));
            roots.push(home.join("snap/steam/common/.local/share/Steam"));
        }
        roots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_client_location_is_reported_for_a_home() {
        let locator =
            PosixSteamLocator::with_home(OperatingSystem::Linux, PathBuf::from("/home/tester"));
        let roots = locator.steam_roots();
        assert!(roots.contains(&PathBuf::from("/home/tester/.local/share/Steam")));
        assert!(roots.contains(&PathBuf::from(
            "/home/tester/.var/app/com.valvesoftware.Steam/data/Steam"
        )));
    }

    #[test]
    fn a_macos_locator_reports_the_steam_client_directories() {
        let locator =
            PosixSteamLocator::with_home(OperatingSystem::MacOS, PathBuf::from("/Users/tester"));
        let roots = locator.steam_roots();
        assert!(roots.contains(&PathBuf::from("/Users/tester/.steam/steam")));
        assert!(!roots
            .iter()
            .any(|root| root.to_string_lossy().contains(".var/app")));
    }
}
