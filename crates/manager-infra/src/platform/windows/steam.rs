//! The Steam client locations a Windows host uses.
//!
//! The registry is authoritative because a user can install Steam anywhere, and
//! the default locations are only a fallback for a broken or missing key. The
//! library list itself lives in libraryfolders.vdf and is read by the shared
//! orchestration.

use crate::platform::steam::SteamRootLocator;
use crate::platform::windows::registry;
use manager_core::game::OperatingSystem;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct WindowsSteamLocator {
    /// Overrides the registry and default locations, for tests.
    explicit_roots: Option<Vec<PathBuf>>,
}

impl WindowsSteamLocator {
    pub fn new() -> Self {
        Self::default()
    }

    /// A locator that reports exactly these roots, for tests and for callers
    /// that already know the Steam installation.
    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self {
            explicit_roots: Some(roots),
        }
    }

    /// The location recorded in the registry, when Steam is installed.
    pub fn registry_root() -> Option<PathBuf> {
        registry::steam_install_path().map(PathBuf::from)
    }

    /// The default install locations, which cover the standard Steam setup and
    /// the "Program Files" variant a custom install may use.
    pub fn default_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(program_files) = std::env::var_os("ProgramFiles(x86)") {
            roots.push(PathBuf::from(program_files).join("Steam"));
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            roots.push(PathBuf::from(program_files).join("Steam"));
        }
        roots
    }
}

impl SteamRootLocator for WindowsSteamLocator {
    fn operating_system(&self) -> OperatingSystem {
        OperatingSystem::Windows
    }

    fn steam_roots(&self) -> Vec<PathBuf> {
        if let Some(roots) = &self.explicit_roots {
            return roots.clone();
        }

        let mut roots = Vec::new();
        if let Some(registry_root) = Self::registry_root() {
            roots.push(registry_root);
        }
        for candidate in Self::default_roots() {
            if !roots.iter().any(|root| root == &candidate) {
                roots.push(candidate);
            }
        }
        roots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_roots_are_reported_verbatim_and_are_windows_candidates() {
        let locator =
            WindowsSteamLocator::with_roots(vec![PathBuf::from(r"D:\SteamLibrary\Steam")]);
        assert_eq!(locator.operating_system(), OperatingSystem::Windows);
        assert_eq!(
            locator.steam_roots(),
            vec![PathBuf::from(r"D:\SteamLibrary\Steam")]
        );
    }

    #[test]
    fn default_roots_are_absolute_windows_paths() {
        for root in WindowsSteamLocator::default_roots() {
            assert!(
                root.to_string_lossy().ends_with("Steam"),
                "unexpected default root {}",
                root.display()
            );
        }
    }
}
