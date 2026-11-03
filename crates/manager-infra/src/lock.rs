//! Cross-process instance exclusion.
//!
//! This lock answers one question: "is another application process using these
//! files?". It is deliberately the coarse, process-level guard, and it is *not*
//! the fine-grained concurrency mechanism - that is the in-process resource
//! coordinator.
//!
//! Clones of one lock share process-local state, so two otherwise-disjoint
//! operations in this process no longer look like another application instance
//! to each other. The OS file lock is taken once, by the first local holder, and
//! released by the last one to drop.

use fs2::FileExt;
use manager_core::ports::InstanceLock;
use std::any::Any;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct LockState {
    /// The open file holding the OS lock, while this process holds it.
    file: Option<File>,
    /// How many guards this process currently holds.
    holders: usize,
}

#[derive(Clone)]
pub struct FileInstanceLock {
    lock_path: PathBuf,
    state: Arc<Mutex<LockState>>,
}

/// One local holder of the process-wide lock.
struct LockGuard {
    state: Arc<Mutex<LockState>>,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.holders = state.holders.saturating_sub(1);
        if state.holders == 0 {
            if let Some(file) = state.file.take() {
                if let Err(error) = file.unlock() {
                    // Unlock failures are advisory: the lock is released when the
                    // file handle closes in every case that matters.
                    eprintln!("failed to release the instance lock: {error}");
                }
            }
        }
    }
}

impl FileInstanceLock {
    pub fn new<P: AsRef<Path>>(lock_path: P) -> Self {
        Self {
            lock_path: lock_path.as_ref().to_path_buf(),
            state: Arc::new(Mutex::new(LockState::default())),
        }
    }
}

impl InstanceLock for FileInstanceLock {
    fn acquire_guard(&self) -> Result<Box<dyn Any + Send + Sync>, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| format!("Instance lock state is poisoned: {}", e))?;

        // Already held in this process: share it instead of trying to take a
        // second OS lock on the same file, which the OS would refuse.
        if state.holders > 0 {
            state.holders += 1;
            return Ok(Box::new(LockGuard {
                state: Arc::clone(&self.state),
            }));
        }

        if let Some(parent) = self.lock_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_path)
            .map_err(|e| {
                format!(
                    "Failed to open lock file '{}': {}",
                    self.lock_path.display(),
                    e
                )
            })?;

        try_exclusive_briefly(&file, PROBE_GRACE).map_err(|e| {
            format!(
                "Could not acquire cross-process lock (another instance of Stardew Mod Manager is operating): {}",
                e
            )
        })?;

        state.file = Some(file);
        state.holders = 1;

        Ok(Box::new(LockGuard {
            state: Arc::clone(&self.state),
        }))
    }

    fn held_by_another_process(&self) -> bool {
        // While this process holds it, no other process can.
        if self.state.lock().map(|s| s.holders > 0).unwrap_or(false) {
            return false;
        }
        is_held(&self.lock_path)
    }
}

/// How long a lock attempt keeps trying before it reports the lock as held.
///
/// Liveness probes (`is_held`) hold the lock for microseconds; a real holder
/// holds it for as long as a change takes. Retrying for a moment means a
/// probe from the command line can never make a real attempt fail.
const PROBE_GRACE: std::time::Duration = std::time::Duration::from_millis(200);

fn try_exclusive_briefly(file: &File, grace: std::time::Duration) -> std::io::Result<()> {
    let started = std::time::Instant::now();
    loop {
        match FileExt::try_lock_exclusive(file) {
            Ok(()) => return Ok(()),
            Err(error) if started.elapsed() >= grace => return Err(error),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
}

/// Whether some process holds the lock file. Never blocks and never creates
/// the file: no file means nobody has ever held it.
fn is_held(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).open(path) else {
        return false;
    };
    match FileExt::try_lock_shared(&file) {
        Ok(()) => {
            let _ = FileExt::unlock(&file);
            false
        }
        Err(_) => true,
    }
}

/// Held by an open app window for as long as it runs.
///
/// The instance lock above is taken only while a change is carried out, so
/// it cannot tell whether the app is open. This one can: a second app started
/// on the same data folder finds it held and does not open (and so never runs
/// startup recovery under a window that is still working), and the command
/// line uses it to tell an operation the app is carrying out right now from
/// one a closed app left behind.
pub struct AppPresenceLock {
    _file: File,
}

impl AppPresenceLock {
    /// Takes the lock, or returns `Ok(None)` when another process holds it.
    pub fn try_hold<P: AsRef<Path>>(lock_path: P) -> Result<Option<Self>, String> {
        let path = lock_path.as_ref();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| format!("Failed to open lock file '{}': {}", path.display(), e))?;
        // A command-line probe holds it for an instant; only a lock that stays
        // held is another window.
        match try_exclusive_briefly(&file, PROBE_GRACE) {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(_) => Ok(None),
        }
    }

    /// Whether an app window currently holds the lock. Never blocks, and
    /// never creates the file: no file means no app has ever held it.
    pub fn is_held_elsewhere<P: AsRef<Path>>(lock_path: P) -> bool {
        is_held(lock_path.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_process_holding_the_instance_lock_is_seen_but_this_one_is_not() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("instance.lock");
        let here = FileInstanceLock::new(&path);
        // An independent lock object models a different process.
        let other = FileInstanceLock::new(&path);
        assert!(!here.held_by_another_process());

        let guard = other.acquire_guard().unwrap();
        assert!(here.held_by_another_process());
        drop(guard);
        assert!(!here.held_by_another_process());

        let _mine = here.acquire_guard().unwrap();
        assert!(
            !here.held_by_another_process(),
            "this process's own hold is not another process"
        );
    }

    #[test]
    fn a_momentary_probe_never_makes_a_real_attempt_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app.lock");
        std::fs::write(&path, b"").unwrap();
        // A probe holding the file (as `is_held` does) while a window starts.
        let probe = OpenOptions::new().read(true).open(&path).unwrap();
        FileExt::try_lock_shared(&probe).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            drop(probe);
        });
        assert!(
            AppPresenceLock::try_hold(&path).unwrap().is_some(),
            "a window must not report 'already open' because of a probe"
        );
        release.join().unwrap();
    }

    #[test]
    fn only_one_app_holds_the_presence_lock_and_others_can_see_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("app.lock");
        assert!(!AppPresenceLock::is_held_elsewhere(&path));

        let first = AppPresenceLock::try_hold(&path).unwrap();
        assert!(first.is_some());
        assert!(
            AppPresenceLock::try_hold(&path).unwrap().is_none(),
            "a second app on the same data folder must not get the lock"
        );
        assert!(AppPresenceLock::is_held_elsewhere(&path));

        drop(first);
        assert!(!AppPresenceLock::is_held_elsewhere(&path));
        assert!(AppPresenceLock::try_hold(&path).unwrap().is_some());
    }

    #[test]
    fn clones_share_the_process_local_lock_and_it_stays_held() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = FileInstanceLock::new(tmp.path().join("instance.lock"));

        // Two local holders from the same shared instance both succeed.
        let first = lock.acquire_guard().expect("first local guard");
        let clone = lock.clone();
        let second = clone.acquire_guard().expect("second local guard");

        // The OS lock is still held, so a genuinely different process - modelled
        // here by an independent lock object - cannot take it.
        let other = FileInstanceLock::new(tmp.path().join("instance.lock"));
        assert!(
            other.acquire_guard().is_err(),
            "another instance must still be excluded while this process holds the lock"
        );

        drop(first);
        assert!(
            other.acquire_guard().is_err(),
            "the OS lock must stay held while any local guard remains"
        );

        drop(second);
        other
            .acquire_guard()
            .expect("the OS lock is released once the last local guard is dropped");
    }

    #[test]
    fn a_dropped_guard_releases_the_os_lock_for_a_new_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = FileInstanceLock::new(tmp.path().join("instance.lock"));
        {
            let _guard = lock.acquire_guard().expect("guard");
        }
        let other = FileInstanceLock::new(tmp.path().join("instance.lock"));
        other.acquire_guard().expect("lock released");
    }
}
