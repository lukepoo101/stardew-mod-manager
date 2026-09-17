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

        file.try_lock_exclusive().map_err(|e| {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
