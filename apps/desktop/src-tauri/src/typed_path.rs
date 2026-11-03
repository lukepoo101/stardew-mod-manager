//! Paths a person typed or pasted.
//!
//! The mod archive field and the game folder field accept the same spellings:
//! surrounding quotes (Windows "Copy as path"), a `file://` link from a file
//! manager, percent-encoded characters in such a link, and `~` for the home
//! directory. Both fields go through these helpers so they cannot drift apart.
//!
//! The rules that differ between Windows and POSIX (drive letters in links,
//! `~\`, network shares) take a [`PathStyle`], chosen once from the host's
//! path semantics, so every host's rules can be tested on any host.

use std::path::{Path, PathBuf};

/// Whose path spelling a typed path is read with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    Windows,
    Posix,
}

impl PathStyle {
    /// The style of the filesystem this build runs on.
    pub fn host() -> Self {
        if manager_core::path_semantics::host_path_semantics().id() == "windows" {
            PathStyle::Windows
        } else {
            PathStyle::Posix
        }
    }
}

/// Why a typed path cannot be used at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedPathError {
    /// A `file://` link naming another computer, which only Windows can
    /// open (as a network share).
    RemoteHost(String),
    /// A NUL character, which no filesystem allows in a name.
    Nul,
}

impl std::fmt::Display for TypedPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypedPathError::RemoteHost(host) => write!(
                f,
                "The link points to a folder on another computer ({host}), which cannot be opened from here. Copy the folder to this computer, or paste its local path."
            ),
            TypedPathError::Nul => write!(f, "The path contains a NUL character."),
        }
    }
}

/// The path inside what was pasted: trimmed, unquoted, and with a `file://`
/// prefix and its percent-encoding removed. `~` is left for [`expand_home`].
pub fn unwrap_pasted(text: &str, style: PathStyle) -> Result<String, TypedPathError> {
    let mut trimmed = text.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        trimmed = trimmed[1..trimmed.len() - 1].trim();
    }

    // The scheme is case-insensitive ("FILE:///x"). The prefix is ASCII, so
    // slicing at its length is on a character boundary.
    let scheme = "file:";
    let is_link = trimmed.len() >= scheme.len()
        && trimmed.is_char_boundary(scheme.len())
        && trimmed[..scheme.len()].eq_ignore_ascii_case(scheme);
    if !is_link {
        return refuse_nul(trimmed.to_string());
    }
    let rest = &trimmed[scheme.len()..];
    let path = match rest.strip_prefix("//") {
        // "file://host/path": an empty host or "localhost" is this computer.
        Some(authority_and_path) => {
            let (host, path) = match authority_and_path.find('/') {
                Some(slash) => authority_and_path.split_at(slash),
                None => (authority_and_path, ""),
            };
            let drive = host.len() == 2
                && host.as_bytes()[0].is_ascii_alphabetic()
                && host.as_bytes()[1] == b':';
            if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
                path.to_string()
            } else if drive && style == PathStyle::Windows {
                // "file://C:/Games", written without the third slash.
                format!("{host}{path}")
            } else if style == PathStyle::Windows {
                // Explorer writes a network share as file://server/share/...
                format!("\\\\{}{}", host, path.replace('/', "\\"))
            } else {
                return Err(TypedPathError::RemoteHost(percent_decode(host)));
            }
        }
        // "file:/home/x", the single-slash form some tools write.
        None => rest.to_string(),
    };
    let decoded = percent_decode(&path);
    // "file:///C:/Games" is the Windows path "C:/Games".
    let bytes = decoded.as_bytes();
    if style == PathStyle::Windows
        && bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
    {
        return refuse_nul(decoded[1..].trim().to_string());
    }
    refuse_nul(decoded.trim().to_string())
}

fn refuse_nul(path: String) -> Result<String, TypedPathError> {
    if path.contains('\0') {
        Err(TypedPathError::Nul)
    } else {
        Ok(path)
    }
}

/// Decodes `%XX` sequences as UTF-8 bytes, so `caf%C3%A9` is `café`. Only two
/// hex digits make a sequence; any other `%` is kept as it is.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a candidate starts with `~` meaning the home directory: `~` alone,
/// `~/`, and on Windows `~\` (on POSIX a backslash is a filename character).
pub fn is_home_relative(candidate: &str, style: PathStyle) -> bool {
    candidate == "~"
        || candidate.starts_with("~/")
        || (style == PathStyle::Windows && candidate.starts_with("~\\"))
}

/// Expands a leading `~` to the home directory. Without a home directory the
/// path is returned unchanged, so the caller's "not found" says what was
/// looked for.
pub fn expand_home(candidate: &str, home: Option<&Path>, style: PathStyle) -> PathBuf {
    match home {
        Some(home) if is_home_relative(candidate, style) => {
            let rest = candidate[1..].trim_start_matches(['/', '\\']);
            if rest.is_empty() {
                home.to_path_buf()
            } else {
                home.join(rest)
            }
        }
        _ => PathBuf::from(candidate),
    }
}

/// A folder path as typed or pasted, ready to inspect.
pub fn folder(
    text: &str,
    home: Option<&Path>,
    style: PathStyle,
) -> Result<PathBuf, TypedPathError> {
    Ok(expand_home(&unwrap_pasted(text, style)?, home, style))
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSIX: PathStyle = PathStyle::Posix;
    const WINDOWS: PathStyle = PathStyle::Windows;

    fn unwrap(text: &str, style: PathStyle) -> String {
        unwrap_pasted(text, style).unwrap()
    }

    #[test]
    fn quoted_linked_and_home_relative_folders_name_the_folder() {
        let home = Path::new("/home/player");
        let want = PathBuf::from("/games/Stardew Valley");
        for typed in [
            "/games/Stardew Valley",
            "  /games/Stardew Valley  ",
            "\"/games/Stardew Valley\"",
            "'/games/Stardew Valley'",
            "file:///games/Stardew%20Valley",
            "FILE:///games/Stardew%20Valley",
            "file://localhost/games/Stardew%20Valley",
            "file:/games/Stardew%20Valley",
        ] {
            assert_eq!(folder(typed, Some(home), POSIX).unwrap(), want, "{typed}");
        }
        assert_eq!(
            folder("~/games/Stardew Valley", Some(home), POSIX).unwrap(),
            PathBuf::from("/home/player/games/Stardew Valley")
        );
        assert_eq!(
            folder("~", Some(home), POSIX).unwrap(),
            PathBuf::from("/home/player")
        );
        assert_eq!(
            folder("\"~/x\"", None, POSIX).unwrap(),
            PathBuf::from("~/x"),
            "without a home the path is left for the error to name"
        );
    }

    #[test]
    fn a_backslash_after_tilde_is_home_only_on_windows() {
        let home = Path::new("/home/player");
        assert_eq!(
            folder("~\\Games", Some(home), POSIX).unwrap(),
            PathBuf::from("~\\Games")
        );
        assert_eq!(
            folder("~\\Games", Some(home), WINDOWS).unwrap(),
            home.join("Games")
        );
    }

    #[test]
    fn percent_encoding_is_decoded_as_utf8() {
        assert_eq!(
            unwrap("file:///mods/caf%C3%A9%20%E6%BC%A2", POSIX),
            "/mods/café 漢"
        );
        assert_eq!(unwrap("file:///a/100%", POSIX), "/a/100%");
        assert_eq!(unwrap("file:///a/%zz", POSIX), "/a/%zz");
        // Only two hex digits make an escape: "+5" is not a number here.
        assert_eq!(unwrap("file:///a/%+5", POSIX), "/a/%+5");
        // Outside a link a percent sign is part of the name.
        assert_eq!(unwrap("/a/50%20off", POSIX), "/a/50%20off");
    }

    #[test]
    fn a_nul_is_refused() {
        assert_eq!(
            unwrap_pasted("file:///a/%00b", POSIX),
            Err(TypedPathError::Nul)
        );
    }

    #[test]
    fn windows_file_links_drop_the_slash_before_the_drive_on_windows_only() {
        assert_eq!(
            unwrap("file:///C:/Program%20Files/Stardew%20Valley", WINDOWS),
            "C:/Program Files/Stardew Valley"
        );
        assert_eq!(unwrap("file:///C:/x", POSIX), "/C:/x");
    }

    #[test]
    fn links_to_another_computer_are_shares_on_windows_and_refused_elsewhere() {
        assert_eq!(
            unwrap("file://server/share/Stardew%20Valley", WINDOWS),
            "\\\\server\\share\\Stardew Valley"
        );
        assert_eq!(
            unwrap_pasted("file://server/share/x", POSIX),
            Err(TypedPathError::RemoteHost("server".into()))
        );
        // "localhostfoo" is a host, not localhost followed by a path.
        assert!(unwrap_pasted("file://localhostfoo/x", POSIX).is_err());
    }

    #[test]
    fn a_lone_quote_is_not_stripped() {
        assert_eq!(unwrap("\"", POSIX), "\"");
    }
}
