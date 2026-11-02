//! Paths a person typed or pasted.
//!
//! The mod archive field and the game folder field accept the same spellings:
//! surrounding quotes (Windows "Copy as path"), a `file://` link from a file
//! manager, percent-encoded characters in such a link, and `~` for the home
//! directory. Both fields go through these helpers so they cannot drift apart.

use std::path::{Path, PathBuf};

/// The path inside what was pasted: trimmed, unquoted, and with a `file://`
/// prefix and its percent-encoding removed. `~` is left for [`expand_home`].
pub fn unwrap_pasted(text: &str) -> String {
    let mut trimmed = text.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        trimmed = trimmed[1..trimmed.len() - 1].trim();
    }

    let Some(link) = trimmed.strip_prefix("file://") else {
        return trimmed.to_string();
    };
    // "file://localhost/x" names the same file as "file:///x".
    let link = link.strip_prefix("localhost").unwrap_or(link);
    let decoded = percent_decode(link);
    // "file:///C:/Games" is the Windows path "C:/Games".
    let bytes = decoded.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return decoded[1..].trim().to_string();
    }
    decoded.trim().to_string()
}

/// Decodes `%XX` sequences as UTF-8 bytes, so `caf%C3%A9` is `café`. A `%` that
/// does not start a valid sequence is kept as it is.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
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

/// Expands a leading `~` to the home directory. Without a home directory the
/// path is returned unchanged, so the caller's "not found" says what was
/// looked for.
pub fn expand_home(candidate: &str, home: Option<&Path>) -> PathBuf {
    if let Some(home) = home {
        if candidate == "~" {
            return home.to_path_buf();
        }
        if let Some(rest) = candidate
            .strip_prefix("~/")
            .or_else(|| candidate.strip_prefix("~\\"))
        {
            return home.join(rest);
        }
    }
    PathBuf::from(candidate)
}

/// A folder path as typed or pasted, ready to inspect.
pub fn folder(text: &str, home: Option<&Path>) -> PathBuf {
    expand_home(&unwrap_pasted(text), home)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "file://localhost/games/Stardew%20Valley",
        ] {
            assert_eq!(folder(typed, Some(home)), want, "{typed}");
        }
        assert_eq!(
            folder("~/games/Stardew Valley", Some(home)),
            PathBuf::from("/home/player/games/Stardew Valley")
        );
        assert_eq!(folder("~", Some(home)), PathBuf::from("/home/player"));
        assert_eq!(
            folder("\"~/x\"", None),
            PathBuf::from("~/x"),
            "without a home the path is left for the error to name"
        );
    }

    #[test]
    fn percent_encoding_is_decoded_as_utf8() {
        assert_eq!(
            unwrap_pasted("file:///mods/caf%C3%A9%20%E6%BC%A2"),
            "/mods/café 漢"
        );
        assert_eq!(unwrap_pasted("file:///a/100%"), "/a/100%");
        assert_eq!(unwrap_pasted("file:///a/%zz"), "/a/%zz");
        // Outside a link a percent sign is part of the name.
        assert_eq!(unwrap_pasted("/a/50%20off"), "/a/50%20off");
    }

    #[test]
    fn windows_file_links_drop_the_slash_before_the_drive() {
        assert_eq!(
            unwrap_pasted("file:///C:/Program%20Files/Stardew%20Valley"),
            "C:/Program Files/Stardew Valley"
        );
    }

    #[test]
    fn a_lone_quote_is_not_stripped() {
        assert_eq!(unwrap_pasted("\""), "\"");
    }
}
