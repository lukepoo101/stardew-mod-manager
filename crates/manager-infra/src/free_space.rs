//! Checks that a drive has room before the manager writes a lot to it.
//!
//! The check is made on the drive that holds the destination, not one global
//! value. When free space cannot be read the check says nothing: the write
//! itself still fails safely if the drive fills up.

use std::path::Path;

/// Room kept free beyond what an operation needs.
pub const MARGIN_BYTES: u64 = 64 * 1024 * 1024;

/// Bytes as a short readable size.
pub fn readable(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// The free space on the drive holding `path` (or its nearest existing
/// parent), if it can be read.
pub fn available(path: &Path) -> Option<u64> {
    let mut candidate = Some(path);
    while let Some(current) = candidate {
        if current.exists() {
            return fs2::available_space(current).ok();
        }
        candidate = current.parent();
    }
    None
}

/// Refuses when the drive holding `path` clearly lacks `needed` bytes plus a
/// margin. `what` names the work, `place` the location, for the message.
pub fn ensure(path: &Path, needed: u64, what: &str, place: &str) -> Result<(), String> {
    let Some(free) = available(path) else {
        return Ok(());
    };
    let wanted = needed.saturating_add(MARGIN_BYTES);
    if free < wanted {
        return Err(format!(
            "Not enough free space for {what}: it needs about {} in {place}, but only {} is free there. Free some space and try again.",
            readable(wanted),
            readable(free)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(readable(512), "512 B");
        assert_eq!(readable(1536), "1.5 KB");
        assert_eq!(readable(64 * 1024 * 1024), "64 MB");
    }

    #[test]
    fn a_missing_folder_is_measured_on_its_nearest_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("not/yet/there");
        assert!(available(&deep).is_some());
        assert!(ensure(&deep, 1, "a test", "the temp folder").is_ok());
        let error = ensure(&deep, u64::MAX / 2, "a test", "the temp folder").unwrap_err();
        assert!(
            error.contains("Not enough free space for a test"),
            "{error}"
        );
        assert!(error.contains("in the temp folder"), "{error}");
    }
}
