//! The game launcher port, backed by the host process backend.
//!
//! The launcher owns one thing: turning a launch specification into a tracked
//! process, and answering whether that process - or an equivalent one started
//! elsewhere - is still running. All platform behaviour lives in the backend.

use crate::platform::process::ProcessBackend;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::launcher::GameLauncherPort;
use manager_core::launch::LaunchSpec;
use std::sync::Arc;

pub struct DetachedGameLauncher {
    backend: Arc<dyn ProcessBackend>,
}

impl DetachedGameLauncher {
    /// The launcher for the running host.
    pub fn new() -> Self {
        Self::with_external_discovery(true)
    }

    /// A launcher that only reports processes it started itself.
    ///
    /// Synthetic lifecycle tests use this so an unrelated process on the host
    /// that happens to share the game's image name cannot change the result.
    pub fn isolated() -> Self {
        Self::with_external_discovery(false)
    }

    pub fn with_external_discovery(discover_external_processes: bool) -> Self {
        Self {
            backend: host_process_backend(discover_external_processes),
        }
    }

    pub fn from_backend(backend: Arc<dyn ProcessBackend>) -> Self {
        Self { backend }
    }
}

impl Default for DetachedGameLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl GameLauncherPort for DetachedGameLauncher {
    fn launch_game(&self, spec: &LaunchSpec) -> AppResult<u32> {
        Ok(self.backend.spawn(spec)?.pid)
    }

    fn is_game_running(&self, specific_pid: Option<u32>) -> bool {
        match specific_pid {
            Some(pid) => {
                if self.backend.identity_for(pid).is_some() {
                    self.backend.is_alive(pid)
                } else {
                    // Nothing can be proven about a bare pid, so it is reported
                    // as running: a caller must not mutate a game directory
                    // while a process may still hold its files.
                    self.backend.unknown_pid_is_alive(pid)
                }
            }
            None => self.backend.any_owned_alive() || self.backend.discover_external(),
        }
    }

    fn terminate_game(&self, specific_pid: Option<u32>) -> AppResult<()> {
        match specific_pid {
            Some(pid) => {
                if self.backend.identity_for(pid).is_none() {
                    return Err(AppError::system(
                        "TERMINATE_FAILED",
                        "Cannot safely stop this process after manager restart. Exit the game from its own menu.",
                    ));
                }
                self.backend.terminate_owned(pid)
            }
            None => self.backend.terminate_all_owned(),
        }
    }
}

/// The process backend for the running host.
#[cfg(target_os = "windows")]
fn host_process_backend(discover_external_processes: bool) -> Arc<dyn ProcessBackend> {
    Arc::new(
        crate::platform::windows::process_backend::WindowsProcessBackend::new(
            discover_external_processes,
        ),
    )
}

/// The process backend for the running host.
#[cfg(unix)]
fn host_process_backend(discover_external_processes: bool) -> Arc<dyn ProcessBackend> {
    Arc::new(
        crate::platform::posix::process_backend::PosixProcessBackend::new(
            discover_external_processes,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_isolated_launcher_reports_no_running_game_without_owned_processes() {
        let launcher = DetachedGameLauncher::isolated();
        assert!(!launcher.is_game_running(None));
    }

    #[test]
    fn a_pid_that_was_never_owned_is_never_terminated() {
        let launcher = DetachedGameLauncher::isolated();
        assert!(launcher.terminate_game(Some(u32::MAX)).is_err());
    }
}
