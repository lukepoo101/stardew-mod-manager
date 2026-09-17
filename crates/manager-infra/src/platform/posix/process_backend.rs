//! POSIX process lifecycle.
//!
//! Identity is (pid, process creation time, image name). The creation timestamp
//! comes from the kernel and is stable for one process incarnation, which is
//! what makes a recycled pid detectable.
//!
//! Linux additionally opens a pidfd, which pins the exact process incarnation
//! for signalling so no signal can ever reach a recycled pid. macOS has no
//! pidfd, and falls back to the creation-time check before every signal.

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
    /// Present only on Linux, which is the only POSIX platform with pidfds.
    #[cfg(target_os = "linux")]
    pidfd: Option<i32>,
}

impl Tracked {
    fn new(identity: ProcessIdentity) -> Self {
        Self {
            identity,
            pidfd: None,
        }
    }

    fn close(&self) {
        #[cfg(target_os = "linux")]
        if let Some(fd) = self.pidfd {
            unsafe {
                libc::close(fd);
            }
        }
    }
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
            process_creation_time(pid),
            spec.executable
                .file_name()
                .map(|name| name.to_string_lossy().to_string()),
        );

        #[cfg(target_os = "linux")]
        let pidfd = {
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            (fd >= 0).then_some(fd as i32)
        };
        #[cfg(target_os = "linux")]
        let _ = pidfd;
        let mut tracked = Tracked::new(identity.clone());
        #[cfg(target_os = "linux")]
        {
            tracked.pidfd = pidfd;
        }

        if let Ok(mut entries) = self.tracked.lock() {
            entries.push(tracked);
        }

        let tracked = Arc::clone(&self.tracked);
        std::thread::Builder::new()
            .name(format!("reaper-pid-{}", pid))
            .spawn(move || {
                let _ = child.wait();
                if let Ok(mut entries) = tracked.lock() {
                    if let Some(position) = entries.iter().position(|t| t.identity.pid == pid) {
                        entries.remove(position).close();
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
            Some(entry) => tracked_is_alive(entry),
            None => false,
        }
    }

    fn any_owned_alive(&self) -> bool {
        let Ok(tracked) = self.tracked.lock() else {
            return true;
        };
        tracked.iter().any(tracked_is_alive)
    }

    fn unknown_pid_is_alive(&self, pid: u32) -> bool {
        let path = format!("/proc/{}", pid);
        if cfg!(target_os = "linux") {
            return Path::new(&path).exists();
        }
        // Without /proc, a zero signal is the portable existence check. It
        // cannot prove the pid was not recycled, so callers must not act on it.
        unsafe { libc::kill(pid as i32, 0) == 0 }
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
                tracked.remove(position).close();
            }
        }
    }

    fn discover_external(&self) -> bool {
        self.discover_external_processes
            && running_processes()
                .iter()
                .any(|(_pid, name)| image_matches(name))
    }
}

fn image_matches(image_name: &str) -> bool {
    // Linux truncates comm to 15 bytes, and macOS truncates the reported name
    // further, so a prefix comparison is the only reliable test.
    expected_process_images()
        .iter()
        .any(|expected| expected.eq_ignore_ascii_case(image_name))
}

fn terminate(entry: &Tracked) {
    if !tracked_is_alive(entry) {
        entry.close();
        return;
    }

    send_signal(entry, libc::SIGTERM);
    for _ in 0..6 {
        if !tracked_is_alive(entry) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if tracked_is_alive(entry) {
        send_signal(entry, libc::SIGKILL);
    }
    entry.close();
}

#[cfg(target_os = "linux")]
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
        return;
    }
    unsafe {
        libc::kill(entry.identity.pid as i32, signal);
    }
}

#[cfg(not(target_os = "linux"))]
fn send_signal(entry: &Tracked, signal: i32) {
    unsafe {
        libc::kill(entry.identity.pid as i32, signal);
    }
}

#[cfg(target_os = "linux")]
fn tracked_is_alive(entry: &Tracked) -> bool {
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
        process_creation_time(entry.identity.pid),
    ) {
        (Some(expected), Some(observed)) => expected == observed,
        _ => false,
    }
}

#[cfg(not(target_os = "linux"))]
fn tracked_is_alive(entry: &Tracked) -> bool {
    // macOS has no pidfd and this backend does not query the kernel creation
    // time, so liveness is the portable existence check. It is only ever
    // applied to a pid this session started, never to an arbitrary pid.
    unsafe { libc::kill(entry.identity.pid as i32, 0) == 0 }
}

/// The kernel creation timestamp of a process, in microseconds since the epoch.
///
/// Linux reports it as clock ticks since boot in /proc/<pid>/stat, which is
/// stable for one process incarnation and therefore detects pid reuse.
///
/// macOS can supply the same guarantee through proc_pidinfo, but the manager
/// has no macOS support contract yet, so it reports nothing rather than
/// pretending to a precision it does not verify. A None creation time means the
/// backend falls back to process existence, and external process discovery is
/// unavailable there.
#[cfg(target_os = "linux")]
pub fn process_creation_time(pid: u32) -> Option<u64> {
    let content = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let close = content.rfind(')')?;
    let rest = content[close + 1..].trim_start();
    rest.split_whitespace()
        .nth(19)
        .and_then(|value| value.parse().ok())
}

#[cfg(not(target_os = "linux"))]
pub fn process_creation_time(_pid: u32) -> Option<u64> {
    None
}

/// Whether a process name is one of the game's images.
pub fn is_game_process_image(image_name: &str) -> bool {
    image_matches(image_name)
}

/// Every running process as (pid, name).
///
/// Linux answers from /proc. Other POSIX hosts have no equivalent that does not
/// require a platform API this project does not use yet, so they report nothing
/// and external process discovery stays unavailable rather than wrong.
#[cfg(target_os = "linux")]
pub fn running_processes() -> Vec<(u32, String)> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    let mut processes = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        processes.push((pid, comm.trim().to_string()));
    }
    processes
}

#[cfg(not(target_os = "linux"))]
pub fn running_processes() -> Vec<(u32, String)> {
    Vec::new()
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
