//! Shows a manager-owned file or folder in the system file manager.
//!
//! Callers resolve the path from the manager's own records; this never takes a
//! path typed by the user.

use std::io;
use std::path::Path;
use std::process::Command;

pub fn reveal_in_file_manager(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} does not exist", path.display()),
        ));
    }
    let mut command = platform_command(path);
    let mut child = command.spawn()?;
    // The file manager is detached from the app, but its launcher must still be
    // reaped so it does not linger as a zombie process.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(target_os = "windows")]
fn platform_command(path: &Path) -> Command {
    let mut command = Command::new("explorer");
    if path.is_dir() {
        command.arg(path);
    } else {
        // Opens the containing folder with the file selected.
        command.arg(format!("/select,{}", path.display()));
    }
    command
}

#[cfg(target_os = "macos")]
fn platform_command(path: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg("-R").arg(path);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_command(path: &Path) -> Command {
    // xdg-open cannot select a file, so a file's folder is opened instead.
    let target = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    let mut command = Command::new("xdg-open");
    command.arg(target);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_path_is_refused_before_anything_is_started() {
        let error = reveal_in_file_manager(Path::new("/definitely/not/here/smm")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
