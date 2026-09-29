//! Host filesystem semantics and the log locator, as one adapter.
//!
//! Both answer "how does this host treat paths", so they are exported together
//! as the host platform services the composition root wires up.

use manager_app::ports::host::{HostPathSemanticsPort, SessionLogLocatorPort};
use manager_core::path_semantics::{host_path_semantics, PathSemantics};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default)]
pub struct HostPathSemantics;

impl HostPathSemantics {
    pub fn new() -> Self {
        Self
    }
}

impl HostPathSemanticsPort for HostPathSemantics {
    fn semantics(&self) -> &'static dyn PathSemantics {
        host_path_semantics()
    }
}

/// The SMAPI log locator for the selected platform.
#[cfg(target_os = "windows")]
pub fn log_locator_for() -> Box<dyn SessionLogLocatorPort> {
    Box::new(crate::platform::windows::log_locator::WindowsSmapiLogLocator::new())
}

/// The SMAPI log locator for the selected platform.
#[cfg(unix)]
pub fn log_locator_for() -> Box<dyn SessionLogLocatorPort> {
    Box::new(crate::platform::posix::log_locator::PosixSmapiLogLocator::new())
}

/// The SMAPI log locator for a named platform.
///
/// This is what lets diagnostics describe where another platform's log would
/// live without running there.
pub fn log_locator_for_platform(
    operating_system: manager_core::game::OperatingSystem,
) -> Box<dyn SessionLogLocatorPort> {
    match operating_system {
        manager_core::game::OperatingSystem::Windows => {
            #[cfg(target_os = "windows")]
            {
                Box::new(crate::platform::windows::log_locator::WindowsSmapiLogLocator::new())
            }
            #[cfg(not(target_os = "windows"))]
            {
                // The Windows log location is a documented data path, so it can
                // be described on any host.
                Box::new(FixedLogLocator(windows_log_path_hint()))
            }
        }
        _ => linux_locator(),
    }
}

/// The POSIX log locator, or a description of its location off-platform.
#[cfg(unix)]
fn linux_locator() -> Box<dyn SessionLogLocatorPort> {
    Box::new(crate::platform::posix::log_locator::PosixSmapiLogLocator::new())
}

#[cfg(not(unix))]
fn linux_locator() -> Box<dyn SessionLogLocatorPort> {
    // Documented location: ~/.config/StardewValley/ErrorLogs/SMAPI-latest.txt
    Box::new(FixedLogLocator(
        PathBuf::from("~/.config")
            .join("StardewValley")
            .join("ErrorLogs")
            .join("SMAPI-latest.txt"),
    ))
}

/// A locator that always reports the same path.
pub struct FixedLogLocator(pub PathBuf);

impl SessionLogLocatorPort for FixedLogLocator {
    fn candidate_log_paths(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }
}

#[cfg(not(target_os = "windows"))]
fn windows_log_path_hint() -> PathBuf {
    // Documented location: %APPDATA%\StardewValley\ErrorLogs\SMAPI-latest.txt
    PathBuf::from("%APPDATA%")
        .join("StardewValley")
        .join("ErrorLogs")
        .join("SMAPI-latest.txt")
}

/// Where SMAPI writes its log on every supported platform.
///
/// Describing another platform's location is not a guess: these are the
/// documented data directories, and a user troubleshooting a Steam Deck from a
/// Windows desktop needs exactly this list.
pub fn known_smapi_log_locations() -> Vec<(String, String)> {
    use manager_core::game::OperatingSystem;
    [
        OperatingSystem::Linux,
        OperatingSystem::Windows,
        OperatingSystem::MacOS,
    ]
    .into_iter()
    .filter_map(|operating_system| {
        let path = default_log_path(operating_system)?;
        Some((
            operating_system.as_key().to_string(),
            path.to_string_lossy().to_string(),
        ))
    })
    .collect()
}

/// Convenience for callers that only want the default log path.
pub fn default_log_path(operating_system: manager_core::game::OperatingSystem) -> Option<PathBuf> {
    log_locator_for_platform(operating_system)
        .candidate_log_paths()
        .into_iter()
        .next()
}
