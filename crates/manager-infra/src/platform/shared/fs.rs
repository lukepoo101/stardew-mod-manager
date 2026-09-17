//! Filesystem operations that behave under Windows contention.
//!
//! Windows makes rename and delete failures much more likely than POSIX does:
//! antivirus scanners, the search indexer, Explorer's thumbnail cache and the
//! game itself all open files without delete sharing for short windows, and the
//! kernel then refuses an otherwise valid move. Those failures are transient
//! and retrying is correct; a permanent failure must still surface, so the
//! retry policy is bounded and every other error class fails immediately.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The bounded retry schedule. Total worst-case wait is under two seconds, which
/// is long enough for a scanner to release a freshly written file and short
/// enough that a genuinely locked file still fails fast enough to be explained.
const RETRY_DELAYS_MS: &[u64] = &[50, 100, 250, 500, 1000];

/// Whether an I/O error is a transient share/lock conflict worth retrying.
pub fn is_transient_contention(error: &io::Error) -> bool {
    // ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION, which Rust surfaces as
    // PermissionDenied or Other depending on the call.
    if cfg!(target_os = "windows") {
        if let Some(code) = error.raw_os_error() {
            if code == 32 || code == 33 {
                return true;
            }
        }
        // ERROR_ACCESS_DENIED is ambiguous: it is also what a real ACL denial
        // looks like, so it is only retried while another attempt is plausible.
        if error.raw_os_error() == Some(5) {
            return true;
        }
    }
    matches!(error.kind(), io::ErrorKind::Interrupted)
}

/// Runs an operation, retrying only transient contention.
pub fn with_contention_retry<T, F>(mut operation: F) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    let mut attempt = 0usize;
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                if attempt >= RETRY_DELAYS_MS.len() || !is_transient_contention(&error) {
                    return Err(error);
                }
                std::thread::sleep(Duration::from_millis(RETRY_DELAYS_MS[attempt]));
                attempt += 1;
            }
        }
    }
}

/// Whether a path needs the Windows extended-length prefix.
///
/// A path longer than MAX_PATH is only usable by the Win32 APIs that opt in,
/// and Rust's standard library does not add the prefix. The manifest and the
/// docs declare long-path support as expected, so paths that need it are
/// rewritten rather than left to fail with a confusing error.
pub fn extended_path(path: &Path) -> PathBuf {
    if !cfg!(target_os = "windows") {
        return path.to_path_buf();
    }
    let rendered = path.to_string_lossy().to_string();
    if rendered.starts_with("\\\\?\\") {
        return path.to_path_buf();
    }
    // The prefix only applies to fully qualified paths, and a UNC path switches
    // from "\\server\share" to "\\?\UNC\server\share".
    if rendered.starts_with("\\\\") {
        return PathBuf::from(format!("\\\\?\\UNC\\{}", rendered.trim_start_matches('\\')));
    }
    if rendered.len() < 250 || !path.is_absolute() {
        return path.to_path_buf();
    }
    if rendered.starts_with("\\\\?\\") {
        return path.to_path_buf();
    }
    PathBuf::from(format!("\\\\?\\{}", rendered))
}

/// Renames a path, converting it first when it exceeds the legacy limit.
pub fn rename_path(from: &Path, to: &Path) -> io::Result<()> {
    with_contention_retry(|| std::fs::rename(extended_path(from), extended_path(to)))
}

/// Removes a file, retrying transient contention.
pub fn remove_file(path: &Path) -> io::Result<()> {
    with_contention_retry(|| std::fs::remove_file(extended_path(path)))
}

/// Removes a directory tree, retrying transient contention.
pub fn remove_dir_all(path: &Path) -> io::Result<()> {
    with_contention_retry(|| std::fs::remove_dir_all(extended_path(path)))
}

/// Creates a directory tree, retrying transient contention.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    with_contention_retry(|| std::fs::create_dir_all(extended_path(path)))
}

/// Copies a file, converting paths that exceed the legacy limit.
pub fn copy_file(from: &Path, to: &Path) -> io::Result<u64> {
    with_contention_retry(|| std::fs::copy(extended_path(from), extended_path(to)))
}

/// Reads a file to a string, converting paths that exceed the legacy limit.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    std::fs::read_to_string(extended_path(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_permanent_error_is_not_retried() {
        let missing = Path::new("this-file-does-not-exist-anywhere");
        let error = remove_file(missing).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!is_transient_contention(&error));
    }

    #[test]
    fn extended_paths_are_only_rewritten_on_windows() {
        let long = PathBuf::from(format!("C:/{}", "a".repeat(400)));
        let converted = extended_path(&long);
        if cfg!(target_os = "windows") {
            assert!(converted.to_string_lossy().starts_with("\\\\?\\"));
        } else {
            assert_eq!(converted, long);
        }

        // A short path is always left alone: the prefix changes how relative
        // components and trailing dots are interpreted.
        let short = PathBuf::from("C:/games/stardew");
        assert_eq!(extended_path(&short), short);
    }

    #[test]
    fn the_retry_schedule_is_bounded() {
        let mut attempts = 0;
        let result: io::Result<()> = with_contention_retry(|| {
            attempts += 1;
            Err(io::Error::from(io::ErrorKind::Interrupted))
        });
        assert!(result.is_err());
        assert_eq!(attempts, RETRY_DELAYS_MS.len() + 1);
    }

    #[test]
    fn a_transient_failure_can_recover_on_a_later_attempt() {
        let mut attempts = 0;
        let result = with_contention_retry(|| {
            attempts += 1;
            if attempts < 3 {
                Err(io::Error::from(io::ErrorKind::Interrupted))
            } else {
                Ok(attempts)
            }
        });
        assert_eq!(result.unwrap(), 3);
    }

    #[test]
    fn a_locked_destination_still_fails_and_leaves_the_source_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("from");
        let to = tmp.path().join("to");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("file.txt"), b"content").unwrap();
        std::fs::create_dir_all(&to).unwrap();

        // A directory rename onto an existing non-empty directory fails on every
        // platform; what matters is that the source survives for recovery.
        std::fs::write(to.join("occupied.txt"), b"occupied").unwrap();
        assert!(rename_path(&from, &to).is_err());
        assert!(from.join("file.txt").exists());
    }
}
