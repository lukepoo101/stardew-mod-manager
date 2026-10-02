//! Checks that a folder can be read and written before setup relies on it.
//!
//! The write check creates a uniquely named temporary file and removes it
//! again, so nothing is left behind. A folder that does not exist yet is
//! checked by whether its nearest existing parent can be written, since that
//! is where it would be created. Passing is not a guarantee: the real write
//! still handles failure.

use std::path::Path;

/// Why a folder cannot be used, in words, or `Ok` if it can.
pub fn probe_read_write(path: &Path) -> Result<(), String> {
    let mut target = path;
    while !target.exists() {
        match target.parent() {
            Some(parent) => target = parent,
            None => return Err("Neither the folder nor any parent of it exists".to_string()),
        }
    }
    if let Err(error) = std::fs::read_dir(target) {
        return Err(format!(
            "{} cannot be read: {error}",
            target.to_string_lossy()
        ));
    }
    match tempfile::Builder::new()
        .prefix(".smm-probe-")
        .tempfile_in(target)
    {
        Ok(file) => {
            drop(file);
            Ok(())
        }
        Err(error) => Err(format!(
            "{} cannot be written: {error}",
            target.to_string_lossy()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_writable_folder_passes_and_is_left_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(probe_read_write(dir.path()).is_ok());
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn a_missing_folder_is_checked_where_it_would_be_created() {
        let dir = tempfile::tempdir().unwrap();
        assert!(probe_read_write(&dir.path().join("not/yet/here")).is_ok());
        assert!(!dir.path().join("not").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_folder_fails_with_the_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        let writable = tempfile::tempfile_in(&locked).is_ok();
        let result = probe_read_write(&locked);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !writable {
            let message = result.unwrap_err();
            assert!(message.contains("locked"));
            assert!(message.contains("cannot be written"));
        }
    }
}
