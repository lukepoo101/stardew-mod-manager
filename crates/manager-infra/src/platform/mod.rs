//! Platform adapters and the host platform composition.
//!
//! This is the only place in the codebase that selects behaviour by target
//! operating system, and it does so at compile time. Everything above it talks
//! to the ports in manager-app, and everything it builds is an implementation
//! of one of those ports.
//!
//! Each host adapter binds platform APIs - libc on POSIX, Win32 on Windows - so
//! only the target's own adapters are compiled. macOS currently reuses the POSIX
//! adapters, because its game layout and process primitives are the same; it has
//! no packaging or acceptance status of its own. Everything that is data rather
//! than an API call (the installation layouts, the Steam VDF reader, the
//! dependency-manifest reader) lives in shared and is available everywhere,
//! which is what lets one host describe another host's installation.

pub mod discovery;
pub mod host_runtime;
pub mod host_semantics;
/// The POSIX adapters, shared by Linux and macOS.
#[cfg(unix)]
pub mod posix;
pub mod process;
pub mod shared;
pub mod steam;
pub mod steam_discovery;
#[cfg(target_os = "windows")]
pub mod windows;

use manager_app::ports::discovery::{GameDiscoveryPort, GameInstallationInspectorPort};
use manager_app::ports::host::{HostPathSemanticsPort, SessionLogLocatorPort};
use manager_app::ports::runtime_layout::GameRuntimePort;
use manager_core::game::OperatingSystem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Everything a platform contributes to the application.
///
/// The composition root builds one of these and hands the pieces to the
/// services, which never learn which host they are running on.
pub struct HostPlatform {
    pub operating_system: OperatingSystem,
    pub discovery: Arc<dyn GameDiscoveryPort>,
    pub inspector: Arc<dyn GameInstallationInspectorPort>,
    pub runtime: Arc<dyn GameRuntimePort>,
    pub log_locator: Arc<dyn SessionLogLocatorPort>,
    pub path_semantics: Arc<dyn HostPathSemanticsPort>,
    /// Whether process ownership should look for games this manager did not start.
    pub process_discover_external: bool,
}

impl HostPlatform {
    /// The adapters for the platform this build is running on.
    #[cfg(target_os = "windows")]
    pub fn for_host() -> Self {
        Self {
            operating_system: OperatingSystem::Windows,
            discovery: Arc::new(steam_discovery::SteamGameDiscovery::windows()),
            inspector: Arc::new(discovery::PlatformGameInspector::new(
                OperatingSystem::Windows,
            )),
            runtime: Arc::new(windows::runtime::WindowsGameRuntime::new()),
            log_locator: Arc::from(host_semantics::log_locator_for()),
            path_semantics: Arc::new(host_semantics::HostPathSemantics::new()),
            process_discover_external: true,
        }
    }

    /// The adapters for the POSIX platforms this build supports.
    ///
    /// Linux and macOS share the POSIX adapters: the game layout is identical,
    /// and both use libc process primitives. macOS has no packaging or
    /// acceptance status of its own, which is a support decision rather than an
    /// architectural one.
    #[cfg(unix)]
    pub fn for_host() -> Self {
        let operating_system = OperatingSystem::host();
        Self {
            operating_system,
            discovery: Arc::new(steam_discovery::SteamGameDiscovery::posix(operating_system)),
            inspector: Arc::new(discovery::PlatformGameInspector::new(operating_system)),
            runtime: Arc::new(posix::runtime::PosixGameRuntime::new(operating_system)),
            log_locator: Arc::from(host_semantics::log_locator_for()),
            path_semantics: Arc::new(host_semantics::HostPathSemantics::new()),
            process_discover_external: true,
        }
    }

    /// The discovered Steam installations, for diagnostics.
    pub fn discovered_steam_installations(&self) -> Vec<PathBuf> {
        self.discovery
            .discover()
            .into_iter()
            .map(|candidate| candidate.path)
            .collect()
    }
}

/// The SMAPI log path a platform uses.
pub fn default_smapi_log_path(operating_system: OperatingSystem) -> Option<PathBuf> {
    host_semantics::default_log_path(operating_system)
}

/// Whether two paths denote the same file on the host.
pub fn host_paths_equivalent(left: &Path, right: &Path) -> bool {
    manager_core::path_semantics::host_path_semantics().paths_equivalent(left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_platform_reports_the_compiled_target() {
        let platform = HostPlatform::for_host();
        assert_eq!(platform.operating_system, OperatingSystem::host());
        assert_eq!(platform.runtime.operating_system(), OperatingSystem::host());
        assert_eq!(
            platform.discovery.operating_system(),
            OperatingSystem::host()
        );
    }

    #[test]
    fn the_log_locator_reports_a_platform_specific_path() {
        let platform = HostPlatform::for_host();
        let candidates = platform.log_locator.candidate_log_paths();
        assert!(!candidates.is_empty());
        for candidate in candidates {
            let rendered = candidate.to_string_lossy().replace('\\', "/");
            assert!(
                rendered.contains("StardewValley") && rendered.contains("ErrorLogs"),
                "unexpected log path {}",
                candidate.display()
            );
        }
    }

    #[test]
    fn path_equivalence_follows_host_semantics() {
        assert!(host_paths_equivalent(Path::new("a/b"), Path::new("a/b")));
        if cfg!(target_os = "windows") {
            assert!(host_paths_equivalent(Path::new("A/B"), Path::new("a/b")));
        } else {
            assert!(!host_paths_equivalent(Path::new("A/B"), Path::new("a/b")));
        }
    }

    #[test]
    fn a_windows_log_path_can_be_described_from_any_host() {
        let rendered = default_smapi_log_path(OperatingSystem::Windows)
            .expect("the Windows log location is documented")
            .to_string_lossy()
            .replace('\\', "/");
        assert!(rendered.ends_with("StardewValley/ErrorLogs/SMAPI-latest.txt"));
    }
}
