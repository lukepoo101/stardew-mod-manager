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
use crate::platform::process::{ProcessBackend, RecordedProcessState};
use manager_app::error::{AppError, AppResult};
use manager_core::launch::{LaunchSpec, ProcessIdentity};
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
    /// Builds an entry on platforms without a pidfd.
    ///
    /// Linux attaches the pidfd instead, so this constructor is unused there.
    #[cfg(not(target_os = "linux"))]
    fn new(identity: ProcessIdentity) -> Self {
        Self {
            identity,
            #[cfg(target_os = "linux")]
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
            // The pidfd pins this exact process incarnation, so a later signal
            // can never reach a recycled pid.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            (fd >= 0).then_some(fd as i32)
        };
        #[cfg(target_os = "linux")]
        let tracked = Tracked {
            identity: identity.clone(),
            pidfd,
        };
        #[cfg(not(target_os = "linux"))]
        let tracked = Tracked::new(identity.clone());

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

    fn identify_recorded(&self, identity: &ProcessIdentity) -> RecordedProcessState {
        // A process this session started is answered from its tracked entry.
        if let Ok(tracked) = self.tracked.lock() {
            if let Some(entry) = tracked
                .iter()
                .find(|entry| entry.identity.pid == identity.pid)
            {
                return if tracked_is_alive(entry) {
                    RecordedProcessState::Running
                } else {
                    RecordedProcessState::Exited
                };
            }
        }

        // Otherwise re-establish the recorded incarnation from the process.
        let Some(observed_creation) = process_creation_time(identity.pid) else {
            return RecordedProcessState::Unknown;
        };
        if let Some(expected) = identity.creation_time {
            return if expected == observed_creation {
                RecordedProcessState::Running
            } else {
                // The pid was recycled: this session's process has ended.
                RecordedProcessState::Exited
            };
        }

        // Without a recorded creation time the pid cannot be proven to be the
        // same process, so the answer stays unknown rather than being assumed.
        RecordedProcessState::Unknown
    }

    fn discover_external(&self) -> bool {
        self.discover_external_processes
            && running_processes()
                .iter()
                .any(|(pid, _)| matches_game_process(*pid))
    }
}

/// Whether a /proc comm value names one of the game's images.
///
/// The kernel truncates comm to 15 bytes, so "StardewModdingAPI" is reported as
/// "StardewModdingA". Comparing the full name against the truncated value never
/// matches, which is what made an externally started SMAPI invisible to the
/// manager. The comparison therefore accepts either the full name or the
/// truncation the kernel would produce.
fn comm_matches(comm: &str) -> bool {
    expected_process_images().iter().any(|expected| {
        comm.eq_ignore_ascii_case(expected) || comm.eq_ignore_ascii_case(truncate_to_comm(expected))
    })
}

/// A name as the kernel would report it in the comm field.
fn truncate_to_comm(image: &str) -> &str {
    const COMM_LIMIT: usize = 15;
    if image.len() <= COMM_LIMIT {
        return image;
    }
    // comm is cut at a byte boundary, so the cut must not split a UTF-8 sequence.
    let mut end = COMM_LIMIT;
    while end > 0 && !image.is_char_boundary(end) {
        end -= 1;
    }
    &image[..end]
}

/// Whether a running process is the game or its mod loader.
///
/// The comm value is the cheap check; the executable path is the stronger one,
/// because it is not truncated and can distinguish a process that merely shares
/// the game's name from the game's own binary.
fn matches_game_process(pid: u32) -> bool {
    let comm = running_processes()
        .into_iter()
        .find(|(candidate, _)| *candidate == pid)
        .map(|(_, name)| name);
    let Some(comm) = comm else {
        return false;
    };
    if !comm_matches(&comm) {
        return false;
    }
    match process_executable_name(pid) {
        Some(image) => comm_matches(&image),
        // The executable path is unavailable for some processes; the comm match
        // is then the only evidence available and is accepted.
        None => true,
    }
}

/// The file name of a process's executable, resolved through the exe link.
#[cfg(target_os = "linux")]
fn process_executable_name(pid: u32) -> Option<String> {
    let resolved = std::fs::read_link(format!("/proc/{}/exe", pid)).ok()?;
    Some(
        resolved
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| resolved.to_string_lossy().to_string()),
    )
}

#[cfg(not(target_os = "linux"))]
fn process_executable_name(_pid: u32) -> Option<String> {
    None
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
    comm_matches(image_name)
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

    /// The regression this guards: the kernel truncates comm to 15 bytes, so a
    /// prefix comparison is required or SMAPI can never be found externally.
    #[test]
    fn a_truncated_comm_value_still_names_the_mod_loader() {
        assert!(
            "StardewModdingAPI".len() > 15,
            "this test is only meaningful while the name exceeds the comm limit"
        );
        assert_eq!(truncate_to_comm("StardewModdingAPI"), "StardewModdingA");
        assert!(
            comm_matches("StardewModdingA"),
            "the kernel's truncation must match"
        );
        assert!(
            comm_matches("StardewModdingAPI"),
            "the full name must match"
        );
        assert!(
            comm_matches("stardewmoddinga"),
            "matching is case-insensitive"
        );
        assert!(
            comm_matches("StardewValley"),
            "a name shorter than the limit matches directly"
        );
    }

    #[test]
    fn an_unrelated_process_name_never_matches() {
        for name in ["bash", "Stardew", "StardewModding", "python3", ""] {
            assert!(
                !comm_matches(name),
                "{name:?} must not be treated as the game"
            );
        }
    }

    #[test]
    fn truncation_never_splits_a_utf8_sequence() {
        // A multi-byte character straddling the limit must not panic or produce
        // an invalid slice.
        let long_multibyte = "StardewModding\u{e9}\u{e9}\u{e9}";
        let truncated = truncate_to_comm(long_multibyte);
        assert!(truncated.len() <= 15);
        assert!(long_multibyte.starts_with(truncated));
    }

    #[test]
    fn a_recorded_identity_reports_running_then_exited() {
        let backend = PosixProcessBackend::new(false);
        let identity = backend.spawn(&sleep_spec("30")).unwrap();
        assert_eq!(
            backend.identify_recorded(&identity),
            RecordedProcessState::Running
        );
        backend.terminate_owned(identity.pid).unwrap();
        // Once the tracked entry is gone the answer comes from the recorded
        // creation time, which no longer matches anything.
        assert_ne!(
            backend.identify_recorded(&identity),
            RecordedProcessState::Running
        );
    }

    #[test]
    fn an_identity_without_a_creation_time_is_unknown_not_running() {
        let backend = PosixProcessBackend::new(false);
        let identity = backend.spawn(&sleep_spec("30")).unwrap();
        backend.forget(identity.pid);
        let without_creation =
            ProcessIdentity::new(identity.pid, None, identity.image_path.clone());
        assert_eq!(
            backend.identify_recorded(&without_creation),
            RecordedProcessState::Unknown,
            "an unprovable identity must not be reported as the same process"
        );
        backend.terminate_all_owned().unwrap();
        let _ = Command::new("kill")
            .arg("-9")
            .arg(identity.pid.to_string())
            .output();
    }

    #[test]
    fn a_recycled_pid_is_not_the_recorded_process() {
        let backend = PosixProcessBackend::new(false);
        let identity = backend.spawn(&sleep_spec("30")).unwrap();
        backend.forget(identity.pid);
        // Same pid, different incarnation, which is exactly a recycled pid.
        let stale = ProcessIdentity::new(
            identity.pid,
            identity.creation_time.map(|t| t.wrapping_add(1)),
            identity.image_path.clone(),
        );
        assert_eq!(
            backend.identify_recorded(&stale),
            RecordedProcessState::Exited
        );
        let _ = Command::new("kill")
            .arg("-9")
            .arg(identity.pid.to_string())
            .output();
    }
}
