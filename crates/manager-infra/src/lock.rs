use fs2::FileExt;
use manager_core::ports::InstanceLock;
use std::any::Any;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct FileInstanceLock {
    lock_path: PathBuf,
}

struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl FileInstanceLock {
    pub fn new<P: AsRef<Path>>(lock_path: P) -> Self {
        Self {
            lock_path: lock_path.as_ref().to_path_buf(),
        }
    }
}

impl InstanceLock for FileInstanceLock {
    fn acquire_guard(&self) -> Result<Box<dyn Any + Send + Sync>, String> {
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

        // Non-blocking try_lock or blocking lock with timeout
        file.try_lock_exclusive().map_err(|e| {
            format!(
                "Could not acquire cross-process lock (another instance of Stardew Mod Manager is operating): {}",
                e
            )
        })?;

        Ok(Box::new(LockGuard { file }))
    }
}
