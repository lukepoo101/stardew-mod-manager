//! POSIX process lifecycle.
//!
//! Identity is (pid, start time, pidfd). The start time guards against PID
//! reuse; the pidfd pins the exact process incarnation for signalling, so no
//! signal can ever reach a recycled pid. Linux exposes both through /proc and
//! the pidfd syscalls; other POSIX systems fall back to the start-time check.

use crate::platform::posix::process::expected_process_images;
use crate::platform::process::{ProcessBackend, ProcessIdentity};
use manager_app::error::{AppError, AppResult};
use manager_core::launch::LaunchSpec;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct Tracked {
    identity: ProcessIdentity,
    pidfd: Option<i32>,
}

pub struct PosixProcessBackend {
    tracked: Arc<Mutex<Vec<Tracked>>>,
    discover_external_processes: bool,
}

impl PosixProcessBackend {
    pub fn new(discover_external_processes: bool) -> Self {
        Self {
            tracked: Arc::new(Mutex::new(Vec::new())),
            discover_external_processes,
        }
    }
}

impl Default for PosixProcessBackend {
    fn default() -> Self {
        Self::new(true)
    }
}

impl ProcessBackend for PosixProcessBackend {
    fn spawn(&self, spec: &LaunchSpec) -> AppResult<ProcessIdentity> {
        let mut cmd = Command::new(&spec.executable);
        cmd.args(&spec.args);
        cmd.current_dir(&spec.working_dir);
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
        // A new process group keeps the game out of the manager's job control,
        // so closing the manager never signals the game.
        cmd.process_group(0);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn().map_err(|error| {
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
        let identity = ProcessIdentity::new(
            pid,
            get_process_starttime(pid),
            spec.executable
                .file_name()
                .map(|name| name.to_string_lossy().to_string()),
        );
        let pidfd = {
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            (fd >= 0).then_some(fd as i32)
        };

        if let Ok(mut tracked) = self.tracked.lock() {
            tracked.push(Tracked {
                identity: identity.clone(),
                pidfd,
            });
        }

        let tracked = Arc::clone(&self.tracked);
        std::thread::Builder::new()
            .name(format!("reaper-pid-{}", pid))
            .spawn(move || {
                let _ = child.wait();
                if let Ok(mut entries) = tracked.lock() {
                    if let Some(position) = entries.iter().position(|t| t.identity.pid == pid) {
                        let entry = entries.remove(position);
                        if let Some(fd) = entry.pidfd {
                            unsafe {
                                libc::close(fd);
                            }
                        }
                    }
                }
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
        let Ok(tracked) = self.tracked.lock() else {
            return true;
        };
        match tracked.iter().find(|entry| entry.identity.pid == pid) {
            Some(entry) => entry_is_alive(entry),
            None => false,
        }
    }

    fn any_owned_alive(&self) -> bool {
        let Ok(tracked) = self.tracked.lock() else {
            return true;
        };
        tracked.iter().any(entry_is_alive)
    }

    fn unknown_pid_is_alive(&self, pid: u32) -> bool {
        Path::new(&format!("/proc/{}", pid)).exists()
    }

    fn identity_for(&self, pid: u32) -> Option<ProcessIdentity> {
        self.tracked
            .lock()
            .ok()?
            .iter()
            .find(|entry| entry.identity.pid == pid)
            .map(|entry| entry.identity.clone())
    }

    fn terminate_owned(&self, pid: u32) -> AppResult<()> {
        let owned = match self.tracked.lock() {
            Ok(mut tracked) => tracked
                .iter()
                .position(|entry| entry.identity.pid == pid)
                .map(|position| tracked.remove(position)),
            Err(_) => None,
        };

        let Some(owned) = owned else {
            return Err(AppError::system(
                "TERMINATE_FAILED",
                "Cannot safely stop this process after manager restart. Exit the game from its own menu.",
            ));
        };

        terminate(&owned);
        Ok(())
    }

    fn terminate_all_owned(&self) -> AppResult<()> {
        let owned: Vec<Tracked> = match self.tracked.lock() {
            Ok(mut tracked) => tracked.drain(..).collect(),
            Err(_) => Vec::new(),
        };
        for entry in owned {
            terminate(&entry);
        }
        Ok(())
    }

    fn forget(&self, pid: u32) {
        if let Ok(mut tracked) = self.tracked.lock() {
            if let Some(position) = tracked.iter().position(|entry| entry.identity.pid == pid) {
                let entry = tracked.remove(position);
                if let Some(fd) = entry.pidfd {
                    unsafe {
                        libc::close(fd);
                    }
                }
            }
        }
    }

    fn discover_external(&self) -> bool {
        self.discover_external_processes && check_process_names(&expected_process_images())
    }
}

fn terminate(entry: &Tracked) {
    if !entry_is_alive(entry) {
        if let Some(fd) = entry.pidfd {
            unsafe {
                libc::close(fd);
            }
        }
        return;
    }

    send_signal(entry, libc::SIGTERM);
    for _ in 0..6 {
        if !entry_is_alive(entry) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if entry_is_alive(entry) {
        send_signal(entry, libc::SIGKILL);
    }
    if let Some(fd) = entry.pidfd {
        unsafe {
            libc::close(fd);
        }
    }
}

fn send_signal(entry: &Tracked, signal: i32) {
    if let Some(fd) = entry.pidfd {
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd,
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            );
        }
    } else {
        unsafe {
            libc::kill(entry.identity.pid as i32, signal);
        }
    }
}

fn entry_is_alive(entry: &Tracked) -> bool {
    if let Some(fd) = entry.pidfd {
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd,
                0,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        return result == 0;
    }
    match (
        entry.identity.creation_time,
        get_process_starttime(entry.identity.pid),
    ) {
        (Some(expected), Some(observed)) => expected == observed,
        _ => false,
    }
}

/// The kernel start-time field of /proc/<pid>/stat, which is stable for one
/// process incarnation and therefore detects PID reuse.
pub fn get_process_starttime(pid: u32) -> Option<u64> {
    let content = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let close = content.rfind(')')?;
    let rest = content[close + 1..].trim_start();
    rest.split_whitespace().nth(19).and_then(|t| t.parse().ok())
}

fn check_process_names(names: &[String]) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let rendered = name.to_string_lossy();
        if !rendered.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        let comm = comm.trim();
        for target in names {
            // Linux truncates comm to 15 bytes.
            let truncated = &target[..target.len().min(15)];
            if comm.eq_ignore_ascii_case(target) || comm.eq_ignore_ascii_case(truncated) {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sleep_spec(seconds: &str) -> LaunchSpec {
        LaunchSpec {
            executable: PathBuf::from("/bin/sleep"),
            args: vec![seconds.to_string()],
            working_dir: PathBuf::from("/"),
            env: Vec::new(),
        }
    }

    #[test]
    fn a_spawned_process_is_alive_and_can_be_terminated() {
        let backend = PosixProcessBackend::new(false);
        let identity = backend.spawn(&sleep_spec("30")).unwrap();
        assert!(backend.is_alive(identity.pid));
        assert!(backend.any_owned_alive());
        backend.terminate_owned(identity.pid).unwrap();
        assert!(!backend.is_alive(identity.pid));
        assert!(!backend.any_owned_alive());
    }

    #[test]
    fn an_untracked_process_is_never_signalled() {
        let backend = PosixProcessBackend::new(false);
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        assert!(backend.identity_for(child.id()).is_none());
        assert!(backend.terminate_owned(child.id()).is_err());
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn a_forgotten_process_is_no_longer_owned() {
        let backend = PosixProcessBackend::new(false);
        let identity = backend.spawn(&sleep_spec("30")).unwrap();
        backend.forget(identity.pid);
        assert!(backend.identity_for(identity.pid).is_none());
        assert!(backend.terminate_owned(identity.pid).is_err());
        assert!(backend.unknown_pid_is_alive(identity.pid));
        backend.terminate_all_owned().unwrap();
    }
}
