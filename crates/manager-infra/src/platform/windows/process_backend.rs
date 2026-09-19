//! Windows process lifecycle.
//!
//! Identity is (pid, creation time, image path). The creation timestamp comes
//! from the kernel and is stable for one process incarnation, so a recycled pid
//! can never be mistaken for the game. The manager also keeps the process
//! handle it opened for the processes it started, which gives a precise liveness
//! answer without polling the process table.

use crate::platform::process::{ProcessBackend, RecordedProcessState};
use crate::platform::windows::process::expected_process_images;
use crate::platform::windows::win32;
use manager_app::error::{AppError, AppResult};
use manager_core::launch::{LaunchSpec, ProcessIdentity};
use manager_core::path_semantics::host_path_semantics;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

struct OwnedProcess {
    identity: ProcessIdentity,
    handle: win32::OwnedHandle,
}

pub struct WindowsProcessBackend {
    owned: Arc<Mutex<Vec<OwnedProcess>>>,
    discover_external_processes: bool,
}

impl WindowsProcessBackend {
    pub fn new(discover_external_processes: bool) -> Self {
        Self {
            owned: Arc::new(Mutex::new(Vec::new())),
            discover_external_processes,
        }
    }
}

impl Default for WindowsProcessBackend {
    fn default() -> Self {
        Self::new(true)
    }
}

impl ProcessBackend for WindowsProcessBackend {
    fn spawn(&self, spec: &LaunchSpec) -> AppResult<ProcessIdentity> {
        let mut cmd = Command::new(&spec.executable);
        cmd.args(&spec.args);
        cmd.current_dir(&spec.working_dir);
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
        // Detached stdio and no job object: closing the manager must never
        // terminate the game, so the child is deliberately not tied to the
        // manager's lifetime.
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let child = cmd.spawn().map_err(|error| {
            AppError::system(
                "LAUNCH_FAILED",
                format!(
                    "Failed to spawn game process '{}': {}",
                    spec.executable.display(),
                    error
                ),
            )
        })?;

        let pid = child.id();
        let Some(handle) = win32::open_process_for_query(pid) else {
            return Err(AppError::system(
                "LAUNCH_FAILED",
                format!(
                    "Started process {} but could not open it to track its identity",
                    pid
                ),
            ));
        };

        let identity = ProcessIdentity::new(
            pid,
            win32::process_creation_time(handle.0),
            win32::process_image_path(handle.0)
                .or_else(|| Some(spec.executable.to_string_lossy().to_string())),
        );

        {
            let mut owned = self.owned.lock().map_err(|_| {
                AppError::system("LAUNCH_FAILED", "Process tracking state is poisoned")
            })?;
            owned.retain(|entry| entry.identity.pid != pid);
            owned.push(OwnedProcess {
                identity: identity.clone(),
                handle,
            });
        }

        // The child is never waited on by the manager: the game must outlive
        // the manager, so this reaper thread only releases the handle Rust keeps
        // internally once the game exits.
        std::thread::Builder::new()
            .name(format!("reaper-pid-{}", pid))
            .spawn(move || {
                let mut child = child;
                let _ = child.wait();
            })
            .map_err(|error| {
                AppError::system(
                    "LAUNCH_FAILED",
                    format!("Failed to spawn process reaper thread: {}", error),
                )
            })?;

        Ok(identity)
    }

    fn is_alive(&self, pid: u32) -> bool {
        let Ok(owned) = self.owned.lock() else {
            return true;
        };
        match owned.iter().find(|entry| entry.identity.pid == pid) {
            Some(entry) => !win32::has_exited(entry.handle.0),
            None => false,
        }
    }

    fn any_owned_alive(&self) -> bool {
        let Ok(owned) = self.owned.lock() else {
            return true;
        };
        owned.iter().any(|entry| !win32::has_exited(entry.handle.0))
    }

    fn unknown_pid_is_alive(&self, pid: u32) -> bool {
        match win32::open_process_for_query(pid) {
            Some(handle) => !win32::has_exited(handle.0),
            // The pid could not be opened, which is what a protected or
            // recently exited process looks like. Nothing is proven either way,
            // so the conservative answer is that something may still be there.
            None => true,
        }
    }

    fn identity_for(&self, pid: u32) -> Option<ProcessIdentity> {
        self.owned
            .lock()
            .ok()?
            .iter()
            .find(|entry| entry.identity.pid == pid)
            .map(|entry| entry.identity.clone())
    }

    fn identify_recorded(&self, identity: &ProcessIdentity) -> RecordedProcessState {
        // A tracked process is answered from the handle this session holds,
        // which is the strongest evidence available.
        if let Ok(owned) = self.owned.lock() {
            if let Some(entry) = owned
                .iter()
                .find(|entry| entry.identity.pid == identity.pid)
            {
                return if win32::has_exited(entry.handle.0) {
                    RecordedProcessState::Exited
                } else {
                    RecordedProcessState::Running
                };
            }
        }

        // Otherwise the recorded identity is re-established from the process
        // itself. A pid that cannot be opened is not assumed to be the game:
        // the answer is "unknown", and the caller decides what that permits.
        let Some(handle) = win32::open_process_for_query(identity.pid) else {
            return RecordedProcessState::Unknown;
        };
        if win32::has_exited(handle.0) {
            return RecordedProcessState::Exited;
        }
        if process_matches_identity(identity) {
            RecordedProcessState::Running
        } else {
            // The pid exists but is a different incarnation, so this session's
            // process has ended.
            RecordedProcessState::Exited
        }
    }

    fn terminate_owned(&self, pid: u32) -> AppResult<()> {
        let entry = match self.owned.lock() {
            Ok(mut owned) => owned
                .iter()
                .position(|entry| entry.identity.pid == pid)
                .map(|position| owned.remove(position)),
            Err(_) => None,
        };

        let Some(entry) = entry else {
            return Err(AppError::system(
                "TERMINATE_FAILED",
                "This process was not started by this manager session, so it will not be stopped. Close the game from its own menu.",
            ));
        };

        // Re-establish the incarnation before acting: the pid may have been
        // recycled since it was recorded.
        if !process_matches_identity(&entry.identity) {
            return Err(AppError::system(
                "TERMINATE_FAILED",
                "The process identity could not be confirmed, so it will not be stopped. Close the game from its own menu.",
            ));
        }

        // Termination deliberately opens its own handle with the rights that
        // call needs. The handle held for tracking was opened for query only,
        // and asking for terminate rights while merely observing a process is
        // what makes a query-only open succeed where a combined one would not.
        let handle = match win32::open_owned_process_for_termination(entry.identity.pid) {
            Some(handle) => handle,
            // The process is already gone, so the postcondition holds.
            None => return Ok(()),
        };

        if win32::has_exited(handle.0) {
            return Ok(());
        }

        if unsafe { win32::TerminateProcess(handle.0, 1) } == 0 {
            return Err(AppError::system(
                "TERMINATE_FAILED",
                format!(
                    "Windows refused to terminate process {} (error {})",
                    entry.identity.pid,
                    unsafe { win32::GetLastError() }
                ),
            ));
        }

        // Wait briefly for the process to actually exit so callers can rely on
        // the postcondition rather than re-polling.
        for _ in 0..20 {
            if win32::has_exited(handle.0) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        Err(AppError::system(
            "TERMINATE_FAILED",
            format!(
                "Process {} did not exit after being asked to terminate",
                entry.identity.pid
            ),
        ))
    }

    fn terminate_all_owned(&self) -> AppResult<()> {
        let pids: Vec<u32> = match self.owned.lock() {
            Ok(owned) => owned.iter().map(|entry| entry.identity.pid).collect(),
            Err(_) => Vec::new(),
        };
        for pid in pids {
            // A process that vanished on its own is not a failure.
            let _ = self.terminate_owned(pid);
        }
        Ok(())
    }

    fn forget(&self, pid: u32) {
        if let Ok(mut owned) = self.owned.lock() {
            owned.retain(|entry| entry.identity.pid != pid);
        }
    }

    fn discover_external(&self) -> bool {
        self.discover_external_processes && find_running_game_process().is_some()
    }
}

/// Whether the process behind an identity is still the same incarnation.
fn process_matches_identity(identity: &ProcessIdentity) -> bool {
    let Some(handle) = win32::open_process_for_query(identity.pid) else {
        return false;
    };
    if win32::has_exited(handle.0) {
        return false;
    }
    if let (Some(expected), Some(observed)) = (
        identity.creation_time,
        win32::process_creation_time(handle.0),
    ) {
        if expected != observed {
            return false;
        }
    }
    if let Some(expected) = identity.image_path.as_deref() {
        if let Some(observed) = win32::process_image_path(handle.0) {
            let expected_path = std::path::Path::new(expected);
            let matches = if expected_path.parent().is_none() {
                win32::image_file_name(&observed)
                    .eq_ignore_ascii_case(win32::image_file_name(expected))
            } else {
                host_path_semantics()
                    .paths_equivalent(expected_path, std::path::Path::new(&observed))
            };
            if !matches {
                return false;
            }
        }
    }
    true
}

/// The pid of a running game or SMAPI process, if there is one.
pub fn find_running_game_process() -> Option<u32> {
    let images = expected_process_images();
    for (pid, image_name) in win32::snapshot_processes() {
        if pid == 0
            || !images
                .iter()
                .any(|expected| expected.eq_ignore_ascii_case(&image_name))
        {
            continue;
        }
        // Confirm the process is real and still running before reporting it, so
        // a stale snapshot entry cannot block a launch.
        if let Some(handle) = win32::open_process_for_query(pid) {
            if !win32::has_exited(handle.0) {
                return Some(pid);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cmd_spec(script: &str) -> LaunchSpec {
        LaunchSpec {
            executable: PathBuf::from(
                std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()),
            ),
            args: vec!["/C".to_string(), script.to_string()],
            working_dir: std::env::temp_dir(),
            env: Vec::new(),
        }
    }

    fn long_running_script() -> &'static str {
        "ping -n 30 127.0.0.1 > nul"
    }

    #[test]
    fn a_spawned_process_is_alive_and_can_be_terminated() {
        let backend = WindowsProcessBackend::new(false);
        let identity = backend.spawn(&cmd_spec(long_running_script())).unwrap();
        assert!(backend.is_alive(identity.pid));
        assert!(backend.any_owned_alive());
        backend.terminate_owned(identity.pid).unwrap();
        assert!(!backend.is_alive(identity.pid));
        assert!(!backend.any_owned_alive());
    }

    #[test]
    fn an_untracked_process_is_never_terminated() {
        let backend = WindowsProcessBackend::new(false);
        let mut child = Command::new(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()))
            .args(["/C", long_running_script()])
            .spawn()
            .unwrap();
        assert!(backend.identity_for(child.id()).is_none());
        assert!(backend.terminate_owned(child.id()).is_err());
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn a_forgotten_process_is_no_longer_owned_and_cannot_be_terminated() {
        let backend = WindowsProcessBackend::new(false);
        let identity = backend.spawn(&cmd_spec(long_running_script())).unwrap();
        backend.forget(identity.pid);
        assert!(backend.identity_for(identity.pid).is_none());
        assert!(backend.terminate_owned(identity.pid).is_err());
        // The reaper thread releases the process handle; the process itself is
        // ended here so the test does not leave it behind.
        let _ = Command::new("taskkill")
            .args(["/PID", &identity.pid.to_string(), "/F"])
            .output();
    }

    #[test]
    fn the_expected_images_cover_both_launchers() {
        let images = expected_process_images();
        assert!(images.iter().any(|image| image == "Stardew Valley.exe"));
        assert!(images.iter().any(|image| image == "StardewModdingAPI.exe"));
    }
}
