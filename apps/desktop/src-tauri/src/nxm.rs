//! `nxm://` links: Nexus Mods' "Mod manager download" button.
//!
//! **Receiving one.** The system starts the manager with the link as its only
//! argument. The manager allows one window per data folder (the presence
//! lock in `lib.rs`), so a second process does not open a window: it writes
//! the link to an inbox folder in the data folder and exits quietly. The open
//! window empties the inbox when it starts and on every tick of the watcher
//! that already looks for outside changes, hands each link to the Nexus
//! service, and brings itself to the front. The link's key never reaches the
//! web view; the window sees only which mod and file it names.
//!
//! **Being the handler.** Nothing registers the manager for `nxm://` links
//! by itself, not even the installer: Vortex and Mod Organizer 2 use them
//! too, and taking them over silently would break those. The user turns it
//! on in Settings, and turning it off gives the links back to whatever had
//! them before, when the manager knows what that was.
//!
//! - Windows: `HKEY_CURRENT_USER\Software\Classes\nxm`, for this user only.
//! - Linux: a hidden `.desktop` file in `$XDG_DATA_HOME/applications` set as
//!   the default for `x-scheme-handler/nxm` with `xdg-mime`.
//!
//! Until the manager is registered with Nexus, only a build that may use a
//! personal API key offers this (see `manager_app::services::nexus`).

use crate::state::AppState;
use manager_app::api::dto::NxmHandlerStatusDto;
use manager_infra::paths::AppPaths;
use std::path::{Path, PathBuf};

/// The longest link accepted from the command line or the inbox.
const MAX_LINK_LEN: usize = 2048;

/// At most this many links are read from the inbox at once; any more wait
/// for the next tick.
const INBOX_BATCH: usize = 20;

/// Where the handler that had `nxm://` links before the manager is kept, so
/// turning the manager off can give them back.
const PREVIOUS_HANDLER_KEY: &str = "nxm_handler.previous";

/// The first `nxm://` link among a process's arguments.
pub fn link_in_args(args: &[String]) -> Option<String> {
    args.iter()
        .skip(1)
        .map(|a| a.trim())
        .find(|a| a.len() <= MAX_LINK_LEN && a.to_ascii_lowercase().starts_with("nxm://"))
        .map(str::to_string)
}

pub fn inbox_dir(paths: &AppPaths) -> PathBuf {
    paths.data_dir().join("incoming-links")
}

/// Leaves a link for the open window. Written under a temporary name and
/// renamed, so the window never reads half of one.
pub fn drop_in_inbox(inbox: &Path, link: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(inbox)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("{stamp:032}-{}", uuid::Uuid::new_v4());
    let partial = inbox.join(format!("{name}.part"));
    std::fs::write(&partial, link.as_bytes())?;
    std::fs::rename(&partial, inbox.join(format!("{name}.link")))
}

/// Takes the links waiting in the inbox, oldest first, deleting each as it
/// is read. Anything that is not a short text file is deleted unread.
pub fn take_inbox(inbox: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(inbox) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "link"))
        .collect();
    files.sort();
    files
        .into_iter()
        .take(INBOX_BATCH)
        .filter_map(|path| {
            let small = std::fs::metadata(&path)
                .map(|m| m.is_file() && m.len() <= MAX_LINK_LEN as u64)
                .unwrap_or(false);
            let text = small.then(|| std::fs::read_to_string(&path).ok()).flatten();
            let _ = std::fs::remove_file(&path);
            text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
        })
        .collect()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Hands every link in the inbox to the Nexus service. Returns whether any
/// arrived, so the caller can refresh the window and bring it forward.
pub fn deliver_inbox(state: &AppState) -> bool {
    let links = take_inbox(&inbox_dir(&state.paths));
    if links.is_empty() {
        return false;
    }
    let installed = crate::commands::active_profile_mods(state);
    for raw in links {
        if let Err(error) = state.nexus.receive_link(&raw, now(), &installed) {
            state.nexus.note_refused_link(&error.summary);
        }
    }
    true
}

// --- Registration -----------------------------------------------------------

/// Whether this build and platform can register the manager for links.
pub fn supported(state: &AppState) -> bool {
    state.nexus.personal_key_allowed() && cfg!(any(windows, target_os = "linux"))
}

/// The program the system should start for a link: the AppImage itself
/// when running from one, otherwise this executable.
fn program() -> std::io::Result<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(appimage));
    }
    std::env::current_exe()
}

pub fn status(state: &AppState) -> NxmHandlerStatusDto {
    if !supported(state) {
        return NxmHandlerStatusDto {
            supported: false,
            registered: false,
            error: None,
        };
    }
    match program().and_then(|program| platform::is_registered(&program)) {
        Ok(registered) => NxmHandlerStatusDto {
            supported: true,
            registered,
            error: None,
        },
        Err(error) => NxmHandlerStatusDto {
            supported: true,
            registered: false,
            error: Some(format!(
                "Whether the manager opens Nexus Mods links could not be read: {error}"
            )),
        },
    }
}

/// Makes the manager the handler for `nxm://` links, or gives them back.
pub fn set_registered(state: &AppState, on: bool) -> NxmHandlerStatusDto {
    use manager_app::ports::repositories::PreferencesRepository;
    if !supported(state) {
        return status(state);
    }
    let result = program().and_then(|program| {
        if on {
            let previous = platform::register(&program)?;
            if let Some(previous) = previous {
                let _ = state.repo.set_preference(PREVIOUS_HANDLER_KEY, &previous);
            }
        } else {
            let previous = state
                .repo
                .get_preference(PREVIOUS_HANDLER_KEY)
                .ok()
                .flatten()
                .filter(|p| !p.is_empty());
            platform::unregister(&program, previous.as_deref())?;
            let _ = state.repo.set_preference(PREVIOUS_HANDLER_KEY, "");
        }
        Ok(())
    });
    let mut now = status(state);
    if let Err(error) = result {
        now.error = Some(format!(
            "The manager could not {} Nexus Mods links: {error}",
            if on { "be set to open" } else { "stop opening" }
        ));
    }
    now
}

#[cfg(target_os = "linux")]
mod platform {
    //! A hidden desktop entry set as the default for `x-scheme-handler/nxm`.

    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub const DESKTOP_FILE: &str = "stardew-mod-manager-nxm.desktop";
    const MIME: &str = "x-scheme-handler/nxm";

    fn applications_dir() -> std::io::Result<PathBuf> {
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|p| !p.is_empty())
                    .map(|home| PathBuf::from(home).join(".local/share"))
            })
            .ok_or_else(|| std::io::Error::other("neither XDG_DATA_HOME nor HOME is set"))?;
        Ok(data_home.join("applications"))
    }

    /// A path as one argument of a desktop entry's `Exec` key: quoted, with
    /// the characters the specification reserves escaped, and `%` doubled so
    /// it is not read as a field code.
    pub fn exec_argument(path: &Path) -> String {
        let mut quoted = String::from('"');
        for c in path.to_string_lossy().chars() {
            match c {
                '"' | '`' | '$' | '\\' => {
                    quoted.push('\\');
                    quoted.push(c);
                }
                '%' => quoted.push_str("%%"),
                _ => quoted.push(c),
            }
        }
        quoted.push('"');
        quoted
    }

    pub fn desktop_entry(program: &Path) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName=Stardew Mod Manager (Nexus Mods links)\nExec={} %u\nMimeType={MIME};\nNoDisplay=true\nTerminal=false\n",
            exec_argument(program)
        )
    }

    fn query() -> std::io::Result<Option<String>> {
        let output = Command::new("xdg-mime")
            .args(["query", "default", MIME])
            .output()?;
        let answer = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!answer.is_empty()).then_some(answer))
    }

    fn set_default(desktop_file: &str) -> std::io::Result<()> {
        let status = Command::new("xdg-mime")
            .args(["default", desktop_file, MIME])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("xdg-mime answered {status}")))
        }
    }

    pub fn is_registered(program: &Path) -> std::io::Result<bool> {
        let file = applications_dir()?.join(DESKTOP_FILE);
        let ours = std::fs::read_to_string(&file)
            .map(|text| text == desktop_entry(program))
            .unwrap_or(false);
        Ok(ours && query()?.as_deref() == Some(DESKTOP_FILE))
    }

    /// Returns the desktop file that had the links before, if another one
    /// did.
    pub fn register(program: &Path) -> std::io::Result<Option<String>> {
        let previous = query().ok().flatten().filter(|p| p != DESKTOP_FILE);
        let dir = applications_dir()?;
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(DESKTOP_FILE), desktop_entry(program))?;
        // Best effort: some desktops read the cache rather than the files.
        let _ = Command::new("update-desktop-database").arg(&dir).status();
        set_default(DESKTOP_FILE)?;
        Ok(previous)
    }

    pub fn unregister(_program: &Path, previous: Option<&str>) -> std::io::Result<()> {
        if query().ok().flatten().as_deref() == Some(DESKTOP_FILE) {
            if let Some(previous) = previous {
                set_default(previous)?;
            }
        }
        let dir = applications_dir()?;
        match std::fs::remove_file(dir.join(DESKTOP_FILE)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let _ = Command::new("update-desktop-database").arg(&dir).status();
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    //! `HKEY_CURRENT_USER\Software\Classes\nxm`, for this user only.

    use std::path::Path;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const CLASS: &str = r"Software\Classes\nxm";
    const COMMAND: &str = r"Software\Classes\nxm\shell\open\command";

    fn command_for(program: &Path) -> String {
        format!("\"{}\" \"%1\"", program.display())
    }

    fn current() -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(COMMAND)
            .ok()?
            .get_value::<String, _>("")
            .ok()
    }

    pub fn is_registered(program: &Path) -> std::io::Result<bool> {
        Ok(current().as_deref() == Some(command_for(program).as_str()))
    }

    /// Returns the command that had the links before, if another one did.
    pub fn register(program: &Path) -> std::io::Result<Option<String>> {
        let ours = command_for(program);
        let previous = current().filter(|c| *c != ours);
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let (class, _) = user.create_subkey(CLASS)?;
        class.set_value("", &"URL:NXM Protocol")?;
        class.set_value("URL Protocol", &"")?;
        let (command, _) = user.create_subkey(COMMAND)?;
        command.set_value("", &ours)?;
        Ok(previous)
    }

    pub fn unregister(program: &Path, previous: Option<&str>) -> std::io::Result<()> {
        if !is_registered(program)? {
            return Ok(());
        }
        let user = RegKey::predef(HKEY_CURRENT_USER);
        match previous {
            Some(previous) => {
                let (command, _) = user.create_subkey(COMMAND)?;
                command.set_value("", &previous.to_string())
            }
            None => match user.delete_subkey_all(CLASS) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            },
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use std::path::Path;

    fn unsupported() -> std::io::Error {
        std::io::Error::other("not supported on this platform")
    }

    pub fn is_registered(_: &Path) -> std::io::Result<bool> {
        Ok(false)
    }

    pub fn register(_: &Path) -> std::io::Result<Option<String>> {
        Err(unsupported())
    }

    pub fn unregister(_: &Path, _: Option<&str>) -> std::io::Result<()> {
        Err(unsupported())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_link_among_the_arguments() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            link_in_args(&args(&["app", "NXM://stardewvalley/mods/1/files/2"])),
            Some("NXM://stardewvalley/mods/1/files/2".into())
        );
        assert_eq!(link_in_args(&args(&["nxm://not-the-program"])), None);
        assert_eq!(link_in_args(&args(&["app", "cli", "status"])), None);
        let long = format!("nxm://{}", "a".repeat(MAX_LINK_LEN));
        assert_eq!(link_in_args(&args(&["app", &long])), None);
    }

    #[test]
    fn the_inbox_hands_links_over_once_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let inbox = tmp.path().join("incoming-links");
        drop_in_inbox(&inbox, "nxm://stardewvalley/mods/1/files/1").unwrap();
        drop_in_inbox(&inbox, "nxm://stardewvalley/mods/1/files/2").unwrap();
        // Left behind by a write that did not finish, and too large.
        std::fs::write(inbox.join("x.part"), "nxm://half").unwrap();
        std::fs::write(inbox.join("z.link"), "a".repeat(MAX_LINK_LEN + 1)).unwrap();
        assert_eq!(
            take_inbox(&inbox),
            vec![
                "nxm://stardewvalley/mods/1/files/1".to_string(),
                "nxm://stardewvalley/mods/1/files/2".to_string()
            ]
        );
        assert!(take_inbox(&inbox).is_empty());
        assert!(!inbox.join("z.link").exists());
        assert!(take_inbox(&tmp.path().join("missing")).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_desktop_entry_quotes_the_program() {
        let entry = platform::desktop_entry(Path::new("/opt/My $Apps/100%/smm"));
        assert!(
            entry.contains("Exec=\"/opt/My \\$Apps/100%%/smm\" %u\n"),
            "{entry}"
        );
        assert!(entry.contains("MimeType=x-scheme-handler/nxm;\n"));
        assert!(entry.contains("NoDisplay=true\n"));
    }
}
