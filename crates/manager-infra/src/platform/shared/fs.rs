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
    windows_extended_path(&path.to_string_lossy(), path.is_absolute())
}

/// The Windows path conversion, as a pure function over the rendered path.
///
/// Kept separate from [`extended_path`] so the rules can be exercised on every
/// build host: the hosts that need them are the ones a POSIX CI job cannot run,
/// and getting the UNC and separator handling wrong there is what produces a
/// path Win32 rejects outright.
pub fn windows_extended_path(rendered: &str, is_absolute: bool) -> PathBuf {
    // Win32 only understands backslash separators, and the extended prefix is
    // not recognised at all next to a forward slash: "\\?\C:/games" is an
    // invalid path rather than a long one. Normalising first also makes the
    // verbatim check below catch the mixed spelling.
    let normalized = rendered.replace('/', "\\");
    if normalized.starts_with("\\\\?\\") {
        return PathBuf::from(normalized);
    }
    // A UNC path becomes "\\?\UNC\server\share" rather than "\\?\server\share",
    // which would name a local directory called "server".
    if let Some(share) = normalized.strip_prefix("\\\\") {
        return PathBuf::from(format!("\\\\?\\UNC\\{}", share));
    }
    // The prefix changes how relative components and trailing dots are read, so
    // a short path is always left in its plain form.
    if normalized.len() < 250 || !is_absolute {
        return PathBuf::from(normalized);
    }
    PathBuf::from(format!("\\\\?\\{}", normalized))
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
            // The verbatim form must use Win32 separators throughout; a forward
            // slash next to the prefix makes the path invalid, not long.
            assert!(!converted.to_string_lossy().contains('/'));
        } else {
            assert_eq!(converted, long);
        }

        // A short path is never given the prefix: it changes how relative
        // components and trailing dots are interpreted.
        let short = PathBuf::from("C:/games/stardew");
        let short_converted = extended_path(&short);
        if cfg!(target_os = "windows") {
            assert_eq!(short_converted, PathBuf::from(r"C:\games\stardew"));
        } else {
            assert_eq!(short_converted, short);
        }
    }

    #[test]
    fn the_windows_conversion_normalizes_separators_and_prefixes() {
        // Exercised on every build host, because the separator and UNC rules are
        // exactly what a POSIX CI job cannot otherwise reach.
        assert_eq!(
            windows_extended_path("C:/games/stardew", true),
            PathBuf::from(r"C:\games\stardew")
        );

        let long = format!("C:/{}", "a".repeat(400));
        assert_eq!(
            windows_extended_path(&long, true),
            PathBuf::from(format!(r"\\?\C:\{}", "a".repeat(400)))
        );

        // A UNC path becomes \\?\UNC\server\share rather than a local
        // directory called "server", whichever separator spelling is used.
        assert_eq!(
            windows_extended_path(r"\\server\share\mods", true),
            PathBuf::from(r"\\?\UNC\server\share\mods")
        );
        assert_eq!(
            windows_extended_path("//server/share/mods", true),
            PathBuf::from(r"\\?\UNC\server\share\mods")
        );

        // An already-extended path is left alone rather than double-prefixed.
        assert_eq!(
            windows_extended_path(r"\\?\C:\games", true),
            PathBuf::from(r"\\?\C:\games")
        );

        // A relative path is never given the prefix, which would change its
        // meaning rather than lengthen it.
        assert_eq!(
            windows_extended_path("mods/content.json", false),
            PathBuf::from(r"mods\content.json")
        );
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
