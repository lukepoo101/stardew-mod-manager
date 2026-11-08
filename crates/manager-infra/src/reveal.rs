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

/// Web pages the manager may open in the user's browser. Anything else is
/// refused, so this can never become a general "open any URL" command.
pub const ALLOWED_PAGES: &[&str] = &["https://smapi.io/log"];

/// Sites mods are published on. The update check gives each mod's page on
/// one of these, and any https page on them may be opened.
pub const MOD_PAGE_HOSTS: &[&str] = &[
    "nexusmods.com",
    "www.nexusmods.com",
    "github.com",
    "moddrop.com",
    "www.moddrop.com",
    "curseforge.com",
    "www.curseforge.com",
    "smapi.io",
    "stardewvalleywiki.com",
    "forums.stardewvalley.net",
];

/// Whether `url` is a page the manager opens: one of `ALLOWED_PAGES`, or a
/// plain https address on a mod site, with no user name, password or port.
pub fn is_known_page(url: &str) -> bool {
    if ALLOWED_PAGES.contains(&url) {
        return true;
    }
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none()
        && parsed
            .host_str()
            .is_some_and(|host| MOD_PAGE_HOSTS.contains(&host.to_ascii_lowercase().as_str()))
}

pub fn open_known_page(url: &str) -> io::Result<()> {
    if !is_known_page(url) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{url} is not a page the manager opens"),
        ));
    }
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("explorer");
        c.arg(url);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    let mut child = command.spawn()?;
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
    fn only_allow_listed_pages_open() {
        let error = open_known_page("https://example.com/").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn mod_pages_on_mod_sites_are_known() {
        assert!(is_known_page("https://smapi.io/log"));
        assert!(is_known_page(
            "https://www.nexusmods.com/stardewvalley/mods/541"
        ));
        assert!(is_known_page(
            "https://github.com/focustense/StardewUI/releases"
        ));
        assert!(is_known_page(
            "https://www.curseforge.com/stardewvalley/mods/x"
        ));
        assert!(!is_known_page(
            "http://www.nexusmods.com/stardewvalley/mods/541"
        ));
        assert!(!is_known_page("https://nexusmods.com.evil.example/"));
        assert!(!is_known_page("https://user@github.com/"));
        assert!(!is_known_page("https://github.com:8443/"));
        assert!(!is_known_page("javascript:alert(1)"));
        assert!(!is_known_page("file:///etc/passwd"));
        assert!(!is_known_page("https://example.com/"));
    }

    #[test]
    fn a_missing_path_is_refused_before_anything_is_started() {
        let error = reveal_in_file_manager(Path::new("/definitely/not/here/smm")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
