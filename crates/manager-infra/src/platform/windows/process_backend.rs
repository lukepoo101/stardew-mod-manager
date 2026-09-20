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

        // Existence is checked before identity. A pid that is gone proves the
        // session ended whatever else is unknown about it, and `Unknown` is
        // reserved for a pid that exists but cannot be shown to be the same
        // incarnation. Collapsing the two is what makes a finished session read
        // as still running, because callers treat anything but `Exited` as a
        // live process holding the launch open.
        if !win32::pid_exists(identity.pid) {
            return RecordedProcessState::Exited;
        }

        // The pid exists, so re-establish the recorded incarnation from the
        // process itself. A pid that cannot be opened is not assumed to be the
        // game: the answer is "unknown", and the caller decides what that
        // permits.
        let Some(handle) = win32::open_process_for_query(identity.pid) else {
            // Alive, but unqueryable - a protected process that may have
            // recycled the pid. Nothing about the identity is proven.
            return RecordedProcessState::Unknown;
        };
        if win32::has_exited(handle.0) {
            return RecordedProcessState::Exited;
        }
        match process_matches_identity(identity) {
            IdentityMatch::Confirmed => RecordedProcessState::Running,
            // The pid exists but belongs to a different incarnation, so this
            // session's process has ended.
            IdentityMatch::Mismatched => RecordedProcessState::Exited,
            // The recorded evidence could not be read. Guessing here would
            // contradict the guarantee this identity exists to provide.
            IdentityMatch::Unverifiable => RecordedProcessState::Unknown,
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
        // recycled since it was recorded. An identity that cannot be checked is
        // refused, not assumed, so this requires positive confirmation.
        if !process_matches_identity(&entry.identity).is_confirmed() {
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

/// What comparing a recorded identity with a live process establishes.
///
/// This is deliberately not a boolean. "The evidence disagrees" and "the
/// evidence is unavailable" are different answers, and collapsing them is how a
/// process-safety check starts failing open: a recorded creation time that can
/// no longer be read is not the same as a creation time that matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityMatch {
    /// Every recorded attribute was checked and agrees.
    Confirmed,
    /// A recorded attribute disagrees, so this is a different process.
    Mismatched,
    /// A recorded attribute could not be read, so nothing is proven.
    Unverifiable,
}

impl IdentityMatch {
    /// Whether the identity is proven to be the same process.
    fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed)
    }
}

/// Compares a recorded identity with the live process behind its pid.
fn process_matches_identity(identity: &ProcessIdentity) -> IdentityMatch {
    let Some(handle) = win32::open_process_for_query(identity.pid) else {
        // The process cannot be observed at all, so nothing is established.
        return IdentityMatch::Unverifiable;
    };
    if win32::has_exited(handle.0) {
        return IdentityMatch::Mismatched;
    }

    let mut outcome = IdentityMatch::Confirmed;

    match (
        identity.creation_time,
        win32::process_creation_time(handle.0),
    ) {
        (Some(expected), Some(observed)) if expected != observed => {
            // The pid was recycled: this is a different incarnation.
            return IdentityMatch::Mismatched;
        }
        (Some(_), None) => {
            // The identity recorded a creation time but it can no longer be
            // read, which is exactly the case that must not be treated as a
            // match.
            outcome = IdentityMatch::Unverifiable;
        }
        _ => {}
    }

    if let Some(expected) = identity.image_path.as_deref() {
        match win32::process_image_path(handle.0) {
            Some(observed) => {
                let expected_path = std::path::Path::new(expected);
                let matches = if expected_path.parent().is_none() {
                    win32::image_file_name(&observed)
                        .eq_ignore_ascii_case(win32::image_file_name(expected))
                } else {
                    host_path_semantics()
                        .paths_equivalent(expected_path, std::path::Path::new(&observed))
                };
                if !matches {
                    return IdentityMatch::Mismatched;
                }
            }
            // A recorded image that cannot be read leaves the identity unproven.
            None => outcome = IdentityMatch::Unverifiable,
        }
    }

    outcome
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
        // A matching image name is evidence on its own. Windows checks the
        // requested rights against the process security descriptor, so
        // OpenProcess can legitimately fail for a process that is running;
        // treating that failure as "not running" would let the manager mutate a
        // game directory while the game is open, which is the failure this
        // check exists to prevent.
        //
        // A handle is therefore only used to strengthen the answer: when it can
        // be opened and shows the process has exited, the snapshot entry was
        // stale and is skipped. Inability to open it proves nothing and does not
        // discard the name evidence.
        match win32::open_process_for_query(pid) {
            Some(handle) if win32::has_exited(handle.0) => continue,
            _ => return Some(pid),
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

    /// The rule the process abstraction promises: identity that cannot be
    /// re-established is unknown, never assumed equal.
    #[test]
    fn an_identity_that_cannot_be_checked_is_not_confirmed() {
        let backend = WindowsProcessBackend::new(false);
        let identity = backend.spawn(&cmd_spec(long_running_script())).unwrap();
        let live_creation = identity
            .creation_time
            .expect("Windows reports a creation time");

        // A recorded creation time that disagrees is a different incarnation.
        let recycled = ProcessIdentity::new(
            identity.pid,
            Some(live_creation.wrapping_add(1)),
            identity.image_path.clone(),
        );
        assert_eq!(
            process_matches_identity(&recycled),
            IdentityMatch::Mismatched,
            "a different creation time must be reported as a different process"
        );

        // An independently owned process whose recorded image does not match the
        // live one is also a different process.
        let wrong_image = ProcessIdentity::new(
            identity.pid,
            Some(live_creation),
            Some(r"C:\not\the\live\image.exe".to_string()),
        );
        assert_eq!(
            process_matches_identity(&wrong_image),
            IdentityMatch::Mismatched
        );

        // The genuine article confirms.
        assert_eq!(
            process_matches_identity(&identity),
            IdentityMatch::Confirmed
        );
        let _ = Command::new("taskkill")
            .args(["/PID", &identity.pid.to_string(), "/F"])
            .output();
    }

    #[test]
    fn a_pid_that_cannot_be_opened_is_unverifiable_not_confirmed() {
        // A pid that does not exist cannot be opened, and an identity that
        // cannot be observed must not be treated as proven.
        let missing = ProcessIdentity::new(u32::MAX, Some(1), None);
        assert_eq!(
            process_matches_identity(&missing),
            IdentityMatch::Unverifiable
        );
        assert!(!process_matches_identity(&missing).is_confirmed());
    }

    /// A pid that no longer exists must prove the session ended. Reporting it
    /// as `Unknown` is what permanently blocks the next launch, because callers
    /// treat every state but `Exited` as a live process.
    #[test]
    fn a_vanished_pid_is_reported_as_exited_not_unknown() {
        let backend = WindowsProcessBackend::new(false);
        let identity = backend.spawn(&cmd_spec(long_running_script())).unwrap();
        backend.forget(identity.pid);

        // End the process behind the manager's back, as closing the game from
        // its own menu or an external task manager would.
        let _ = Command::new("taskkill")
            .args(["/PID", &identity.pid.to_string(), "/F"])
            .output();
        for _ in 0..40 {
            if !win32::pid_exists(identity.pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            !win32::pid_exists(identity.pid),
            "the process must actually be gone for this assertion to mean anything"
        );

        assert_eq!(
            backend.identify_recorded(&identity),
            RecordedProcessState::Exited,
            "a vanished pid must not read as an unknown live process"
        );
    }

    #[test]
    fn termination_refuses_an_identity_that_cannot_be_confirmed() {
        let backend = WindowsProcessBackend::new(false);
        let identity = backend.spawn(&cmd_spec(long_running_script())).unwrap();
        backend.forget(identity.pid);

        // Same pid, stale creation time: the process is real, but it cannot be
        // proven to be the one this session started.
        let recycled = ProcessIdentity::new(
            identity.pid,
            identity.creation_time.map(|t| t.wrapping_add(1)),
            identity.image_path.clone(),
        );
        assert!(backend.terminate_owned(recycled.pid).is_err());

        let _ = Command::new("taskkill")
            .args(["/PID", &identity.pid.to_string(), "/F"])
            .output();
    }

    /// Discovery must not treat an unopenable process as an absent one.
    #[test]
    fn a_matching_process_that_cannot_be_opened_still_counts_as_running() {
        // The snapshot evidence is what the answer rests on, so a process whose
        // name matches is reported whether or not a handle can be acquired. This
        // asserts the shape of the rule rather than a denied handle, which a
        // normal test process cannot provoke.
        let images = expected_process_images();
        assert!(
            images
                .iter()
                .any(|expected| expected.eq_ignore_ascii_case("Stardew Valley.exe")),
            "the game's image must be recognised by name"
        );
    }
}
