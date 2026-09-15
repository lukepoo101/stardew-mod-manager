use manager_core::launch::LaunchSpec;
use manager_core::ports::GameLauncher;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone)]
struct TrackedProcess {
    pid: u32,
    #[cfg(target_os = "linux")]
    starttime: u64,
    #[cfg(target_os = "linux")]
    pidfd: Option<i32>,
}

pub struct DetachedGameLauncher {
    active_processes: Arc<Mutex<Vec<TrackedProcess>>>,
    discover_external_processes: bool,
    fail_closed_on_unsupported_platform: bool,
}

impl DetachedGameLauncher {
    /// Isolate synthetic lifecycle tests from unrelated processes on the host.
    pub fn isolated() -> Self {
        Self {
            discover_external_processes: false,
            fail_closed_on_unsupported_platform: false,
            ..Self::new()
        }
    }
    pub fn new() -> Self {
        Self {
            active_processes: Arc::new(Mutex::new(Vec::new())),
            discover_external_processes: true,
            fail_closed_on_unsupported_platform: true,
        }
    }
}

impl Default for DetachedGameLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl GameLauncher for DetachedGameLauncher {
    fn launch_game(&self, spec: &LaunchSpec) -> Result<u32, String> {
        let mut cmd = Command::new(&spec.executable);
        cmd.args(&spec.args);
        cmd.current_dir(&spec.working_dir);

        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        // Complete process detachment where the platform supports process
        // groups. Windows uses detached stdio below and does not expose the
        // Unix process_group extension.
        #[cfg(unix)]
        cmd.process_group(0);

        // 2. Detach all stdio
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let mut child = cmd.spawn().map_err(|e| {
            format!(
                "Failed to spawn game process '{}': {}",
                spec.executable.display(),
                e
            )
        })?;

        let pid = child.id();
        #[cfg(target_os = "linux")]
        let starttime = get_process_starttime(pid).unwrap_or(0);

        #[cfg(target_os = "linux")]
        let pidfd = {
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            (fd >= 0).then_some(fd as i32)
        };
        let tracked = TrackedProcess {
            pid,
            #[cfg(target_os = "linux")]
            starttime,
            #[cfg(target_os = "linux")]
            pidfd,
        };

        if let Ok(mut procs) = self.active_processes.lock() {
            procs.push(tracked);
        }

        // 3. Child tracking and reaping thread
        let active_procs = Arc::clone(&self.active_processes);
        thread::Builder::new()
            .name(format!("reaper-pid-{}", pid))
            .spawn(move || {
                let _ = child.wait();
                if let Ok(mut procs) = active_procs.lock() {
                    if let Some(pos) = procs.iter().position(|p| p.pid == pid) {
                        #[cfg(target_os = "linux")]
                        {
                            let proc = procs.remove(pos);
                            if let Some(fd) = proc.pidfd {
                                unsafe {
                                    libc::close(fd);
                                }
                            }
                        }
                        #[cfg(not(target_os = "linux"))]
                        let _ = procs.remove(pos);
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn process reaper thread: {}", e))?;

        Ok(pid)
    }

    #[allow(unreachable_code)]
    fn is_game_running(&self, specific_pid: Option<u32>) -> bool {
        // The cross-platform backend is not implemented yet. Treat unknown
        // process state as running so callers cannot mutate a game directory
        // while a process may still be alive.
        #[cfg(not(target_os = "linux"))]
        if self.fail_closed_on_unsupported_platform {
            let _ = specific_pid;
            return true;
        }
        if let Some(pid) = specific_pid {
            if let Ok(procs) = self.active_processes.lock() {
                if let Some(proc) = procs.iter().find(|p| p.pid == pid) {
                    return is_tracked_alive(proc);
                }
            }
            return is_pid_alive_simple(pid);
        }

        if let Ok(procs) = self.active_processes.lock() {
            for proc in procs.iter() {
                if is_tracked_alive(proc) {
                    return true;
                }
            }
        }

        self.discover_external_processes
            && check_process_names(&["StardewModdingAPI", "StardewValley"])
    }

    #[allow(unreachable_code)]
    fn terminate_game(&self, specific_pid: Option<u32>) -> Result<(), String> {
        #[cfg(not(target_os = "linux"))]
        if self.fail_closed_on_unsupported_platform {
            let _ = specific_pid;
            return Err("Process lifecycle management is not yet implemented for this platform; refusing to terminate or claim process state".into());
        }
        if let Some(pid) = specific_pid {
            let proc_opt = if let Ok(mut procs) = self.active_processes.lock() {
                procs
                    .iter()
                    .position(|p| p.pid == pid)
                    .map(|pos| procs.remove(pos))
            } else {
                None
            };

            if let Some(proc) = proc_opt {
                send_termination_signals(&proc);
                #[cfg(target_os = "linux")]
                if let Some(fd) = proc.pidfd {
                    unsafe {
                        libc::close(fd);
                    }
                }
            } else {
                return Err("Cannot safely stop this process after manager restart. Exit the game from its own menu.".into());
            }
        } else {
            let procs: Vec<TrackedProcess> = if let Ok(mut guard) = self.active_processes.lock() {
                guard.drain(..).collect()
            } else {
                Vec::new()
            };

            for proc in procs {
                send_termination_signals(&proc);
                #[cfg(target_os = "linux")]
                if let Some(fd) = proc.pidfd {
                    unsafe {
                        libc::close(fd);
                    }
                }
            }
        }

        Ok(())
    }
}

impl manager_app::ports::launcher::GameLauncherPort for DetachedGameLauncher {
    fn launch_game(&self, spec: &LaunchSpec) -> manager_app::error::AppResult<u32> {
        manager_core::ports::GameLauncher::launch_game(self, spec)
            .map_err(|e| manager_app::error::AppError::system("LAUNCH_FAILED", e))
    }

    fn is_game_running(&self, pid: Option<u32>) -> bool {
        manager_core::ports::GameLauncher::is_game_running(self, pid)
    }

    fn terminate_game(&self, pid: Option<u32>) -> manager_app::error::AppResult<()> {
        manager_core::ports::GameLauncher::terminate_game(self, pid)
            .map_err(|e| manager_app::error::AppError::system("TERMINATE_FAILED", e))
    }
}

#[cfg(target_os = "linux")]
fn send_termination_signals(proc: &TrackedProcess) {
    if !is_tracked_alive(proc) {
        return;
    }

    // Try SIGTERM first
    if let Some(fd) = proc.pidfd {
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd,
                libc::SIGTERM,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            );
        }
    } else {
        unsafe {
            libc::kill(proc.pid as i32, libc::SIGTERM);
        }
    }

    // Wait up to 300ms
    for _ in 0..6 {
        if !is_tracked_alive(proc) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Escalate to SIGKILL if still alive
    if is_tracked_alive(proc) {
        if let Some(fd) = proc.pidfd {
            unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    fd,
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                );
            }
        } else {
            unsafe {
                libc::kill(proc.pid as i32, libc::SIGKILL);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn send_termination_signals(_proc: &TrackedProcess) {}

#[cfg(target_os = "linux")]
fn is_tracked_alive(proc: &TrackedProcess) -> bool {
    if let Some(fd) = proc.pidfd {
        let res = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd,
                0, // Null signal to check process existence
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        res == 0
    } else {
        if let Some(start) = get_process_starttime(proc.pid) {
            start == proc.starttime
        } else {
            false
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn is_tracked_alive(_proc: &TrackedProcess) -> bool {
    true
}

#[cfg(target_os = "linux")]
fn is_pid_alive_simple(pid: u32) -> bool {
    let proc_path = format!("/proc/{}", pid);
    Path::new(&proc_path).exists()
}

#[cfg(target_os = "linux")]
pub fn get_process_starttime(pid: u32) -> Option<u64> {
    let stat_path = format!("/proc/{}/stat", pid);
    let content = std::fs::read_to_string(stat_path).ok()?;
    let rparen = content.rfind(')')?;
    let rest = content[rparen + 1..].trim_start();
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    if tokens.len() > 19 {
        tokens[19].parse::<u64>().ok()
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn check_process_names(names: &[&str]) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if name_str.chars().all(|c| c.is_ascii_digit()) {
            let comm_path = entry.path().join("comm");
            if let Ok(comm) = std::fs::read_to_string(&comm_path) {
                let trimmed = comm.trim();
                for &target in names {
                    if trimmed.eq_ignore_ascii_case(target)
                        || (target.len() >= 15 && trimmed.eq_ignore_ascii_case(&target[..15]))
                    {
                        return true;
                    }
                }
            }
        }
    }

    false
}

#[cfg(not(target_os = "linux"))]
fn is_pid_alive_simple(_pid: u32) -> bool {
    false
}

#[cfg(not(target_os = "linux"))]
fn check_process_names(_names: &[&str]) -> bool {
    false
}

#[cfg(all(test, not(target_os = "linux")))]
#[test]
fn unsupported_platform_process_state_fails_closed() {
    let launcher = DetachedGameLauncher::new();
    assert!(launcher.is_game_running(None));
    assert!(launcher.terminate_game(None).is_err());
}
