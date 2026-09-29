//! Host path semantics as an explicit, testable model.
//!
//! Windows and POSIX disagree about more than the separator character, and
//! those disagreements are load-bearing for mod packaging: an archive that is
//! safe on ext4 can still escape, alias, or overwrite a different file on NTFS.
//! The domain therefore carries the rules rather than the host filesystem:
//!
//! * Windows treats names case-insensitively, reserves characters and device
//!   names, strips trailing spaces and dots, and interprets a colon as an
//!   alternate data stream rather than a filename character.
//! * POSIX treats almost every byte sequence as a legal filename.
//!
//! Every rule here is a pure function so both rule sets can be exercised on any
//! build host; the host_path_semantics helper only selects which one the
//! current build should apply by default.

use std::borrow::Cow;
use std::path::Path;

/// Characters Win32 forbids inside a file name.
const WINDOWS_FORBIDDEN_CHARACTERS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Reserved DOS device names. Windows resolves these (optionally with any
/// extension) to a device instead of a file in the directory.
///
/// The console aliases (`CONIN$`, `CONOUT$`) and the legacy clock device
/// (`CLOCK$`) are reserved the same way as `CON` and `NUL`, and an archive that
/// contains one is just as unextractable on Windows.
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM0", "LPT0", "CONIN$", "CONOUT$", "CLOCK$",
];

/// Removes a Win32 verbatim (`\\?\`) prefix from a rendered path.
///
/// The prefix only tells Win32 to skip its own normalisation; it does not name
/// a different file. `\\?\C:\Games` and `C:\Games` are one directory, and the
/// UNC form `\\?\UNC\server\share` is the same share as `\\server\share`, so a
/// comparison that kept the prefix would report a canonicalized root as a
/// second installation of the same game.
fn strip_verbatim_prefix(rendered: &str) -> Cow<'_, str> {
    let Some(rest) = strip_prefix_ignore_ascii_case(rendered, r"\\?\") else {
        return Cow::Borrowed(rendered);
    };
    if let Some(share) = strip_prefix_ignore_ascii_case(rest, r"UNC\") {
        // A verbatim UNC path drops the two leading separators of the standard
        // form, so they are restored rather than left to become components.
        return Cow::Owned(format!(r"\\{}", share));
    }
    Cow::Borrowed(rest)
}

/// `str::strip_prefix`, ignoring ASCII case, as Win32 does for `\\?\` and `UNC`.
fn strip_prefix_ignore_ascii_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// Why a path component cannot be used on a target filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathRuleViolation {
    /// The component is empty.
    Empty,
    /// The component contains a character the filesystem cannot store.
    ForbiddenCharacter(char),
    /// The component names a reserved device rather than a file.
    ReservedDeviceName(String),
    /// The component ends in a space that Win32 would strip or mis-handle.
    TrailingSpace,
    /// The component ends in a dot that Win32 would strip.
    TrailingDot,
    /// The component contains whitespace immediately before a dot, which Win32
    /// normalises away and therefore aliases another name.
    SpaceBeforeExtension,
    /// The component contains a control character.
    ControlCharacter,
}

impl std::fmt::Display for PathRuleViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "the name is empty"),
            Self::ForbiddenCharacter(character) => {
                write!(
                    formatter,
                    "the character '{}' is not allowed in a file name",
                    character
                )
            }
            Self::ReservedDeviceName(name) => write!(
                formatter,
                "'{}' is a reserved Windows device name and cannot be used as a file or folder",
                name
            ),
            Self::TrailingSpace => {
                write!(
                    formatter,
                    "the name ends with a space, which Windows strips"
                )
            }
            Self::TrailingDot => {
                write!(formatter, "the name ends with a dot, which Windows strips")
            }
            Self::SpaceBeforeExtension => write!(
                formatter,
                "the name has a space before its extension, which Windows strips"
            ),
            Self::ControlCharacter => write!(formatter, "the name contains a control character"),
        }
    }
}

/// The comparison identity of path components on one filesystem.
///
/// Two components with the same key are the same file. This deliberately does
/// not know about the current host so a POSIX build can still reason about the
/// archives it would produce on Windows.
pub trait PathSemantics: Send + Sync + std::fmt::Debug {
    /// Stable identifier used in messages and diagnostics.
    fn id(&self) -> &'static str;

    /// Whether component names compare case-insensitively.
    fn is_case_insensitive(&self) -> bool;

    /// Rejects a single path component for this filesystem's namespace.
    fn validate_component(&self, component: &str) -> Result<(), PathRuleViolation>;

    /// The comparison key for one already-validated component.
    fn component_key(&self, component: &str) -> String;

    /// The comparison key for an archive entry name.
    ///
    /// Entry names are always slash-separated and are not host paths, so they
    /// are split explicitly rather than parsed with the host's path rules -
    /// on Windows a name like "a\\b.json" must not become one component.
    fn entry_key(&self, entry_name: &str) -> String {
        entry_name
            .split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .map(|part| self.component_key(part))
            .collect::<Vec<_>>()
            .join("/")
    }

    /// The comparison key for a whole relative path.
    ///
    /// The key is a plain string so callers can use it as a set key for
    /// collision detection, and so persisted forms stay readable.
    fn comparison_key(&self, path: &Path) -> String {
        let mut key = String::new();
        for component in path.components() {
            let raw = component.as_os_str().to_string_lossy();
            if raw.is_empty() {
                continue;
            }
            if !key.is_empty() {
                key.push('/');
            }
            key.push_str(&self.component_key(&raw));
        }
        key
    }

    /// Whether two paths denote the same file under these semantics.
    fn paths_equivalent(&self, left: &Path, right: &Path) -> bool {
        self.comparison_key(left) == self.comparison_key(right)
    }
}

/// POSIX semantics: names are byte strings with almost no structure.
#[derive(Debug, Clone, Copy, Default)]
pub struct PosixPathSemantics;

impl PathSemantics for PosixPathSemantics {
    fn id(&self) -> &'static str {
        "posix"
    }

    fn is_case_insensitive(&self) -> bool {
        false
    }

    fn validate_component(&self, component: &str) -> Result<(), PathRuleViolation> {
        if component.is_empty() {
            return Err(PathRuleViolation::Empty);
        }
        // A NUL or a newline is legal on ext4 but is never something a mod
        // package should rely on.
        if component.chars().any(char::is_control) {
            return Err(PathRuleViolation::ControlCharacter);
        }
        Ok(())
    }

    fn component_key(&self, component: &str) -> String {
        component.to_string()
    }
}

/// Windows semantics: the Win32 file-naming rules as NTFS applies them.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsPathSemantics;

impl WindowsPathSemantics {
    /// Whether a name resolves to a reserved DOS device.
    ///
    /// Windows checks the stem, so CON.txt is still the console device.
    pub fn is_reserved_device_name(name: &str) -> bool {
        let stem = name.split('.').next().unwrap_or(name);
        let upper = stem.trim_end().to_ascii_uppercase();
        if WINDOWS_RESERVED_NAMES.contains(&upper.as_str()) {
            return true;
        }
        if upper.len() == 4 {
            let (prefix, digit) = upper.split_at(3);
            if (prefix == "COM" || prefix == "LPT") && digit.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
        false
    }
}

impl PathSemantics for WindowsPathSemantics {
    fn id(&self) -> &'static str {
        "windows"
    }

    fn is_case_insensitive(&self) -> bool {
        true
    }

    fn validate_component(&self, component: &str) -> Result<(), PathRuleViolation> {
        if component.is_empty() {
            return Err(PathRuleViolation::Empty);
        }
        for character in component.chars() {
            if (character as u32) < 0x20 {
                return Err(PathRuleViolation::ControlCharacter);
            }
            if WINDOWS_FORBIDDEN_CHARACTERS.contains(&character) {
                return Err(PathRuleViolation::ForbiddenCharacter(character));
            }
        }
        if component.ends_with(' ') {
            return Err(PathRuleViolation::TrailingSpace);
        }
        if component.ends_with('.') {
            return Err(PathRuleViolation::TrailingDot);
        }
        // "foo .txt" and "foo.txt" name the same file because Win32 trims the
        // space before the extension.
        if component
            .split('.')
            .any(|segment| segment.ends_with(' ') && !segment.is_empty())
        {
            return Err(PathRuleViolation::SpaceBeforeExtension);
        }
        if Self::is_reserved_device_name(component) {
            return Err(PathRuleViolation::ReservedDeviceName(component.to_string()));
        }
        Ok(())
    }

    fn component_key(&self, component: &str) -> String {
        // Windows case folding for file names is ordinal up to locale; the
        // Turkish-i problem is real but a package that depends on it is broken
        // on the platform regardless, so Unicode lowercase is the rule here.
        component.to_lowercase()
    }

    fn comparison_key(&self, path: &Path) -> String {
        // This must not use the host's path parser: it runs on POSIX too, where
        // a backslash is an ordinary filename byte and `Path::components` would
        // return `C:\Games\Stardew` as one component instead of three. Splitting
        // the rendered path on both separators, after the verbatim prefix is
        // removed, gives the same key on every host.
        let rendered = path.to_string_lossy();
        strip_verbatim_prefix(&rendered)
            .split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .map(|part| self.component_key(part))
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// The semantics of the filesystem this build is running on.
pub fn host_path_semantics() -> &'static dyn PathSemantics {
    static WINDOWS: WindowsPathSemantics = WindowsPathSemantics;
    static POSIX: PosixPathSemantics = PosixPathSemantics;
    if cfg!(target_os = "windows") {
        &WINDOWS
    } else {
        &POSIX
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn windows_rejects_reserved_device_names_with_or_without_extensions() {
        let semantics = WindowsPathSemantics;
        for name in [
            "CON",
            "con",
            "Nul",
            "COM1",
            "lpt9",
            "PRN.txt",
            "AUX.log",
            "com0",
            "CONIN$",
            "conout$.log",
            "CLOCK$",
        ] {
            assert!(
                matches!(
                    semantics.validate_component(name),
                    Err(PathRuleViolation::ReservedDeviceName(_))
                ),
                "{} must be rejected as a reserved device name",
                name
            );
        }
        for name in ["CONSOLE.txt", "COM10.dll", "console"] {
            assert!(
                semantics.validate_component(name).is_ok(),
                "{} is an ordinary file name",
                name
            );
        }
    }

    #[test]
    fn windows_rejects_trailing_spaces_dots_and_alternate_data_streams() {
        let semantics = WindowsPathSemantics;
        assert_eq!(
            semantics.validate_component("config.json "),
            Err(PathRuleViolation::TrailingSpace)
        );
        assert_eq!(
            semantics.validate_component("config.json."),
            Err(PathRuleViolation::TrailingDot)
        );
        assert_eq!(
            semantics.validate_component("config .json"),
            Err(PathRuleViolation::SpaceBeforeExtension)
        );
        assert_eq!(
            semantics.validate_component("config.json:stream"),
            Err(PathRuleViolation::ForbiddenCharacter(':'))
        );
        assert_eq!(
            semantics.validate_component("bad|name"),
            Err(PathRuleViolation::ForbiddenCharacter('|'))
        );
        assert_eq!(
            semantics.validate_component("bad?name"),
            Err(PathRuleViolation::ForbiddenCharacter('?'))
        );
        assert_eq!(
            semantics.validate_component("bad\u{7}name"),
            Err(PathRuleViolation::ControlCharacter)
        );
        assert!(semantics.validate_component("content.json").is_ok());
    }

    #[test]
    fn windows_comparison_keys_fold_case_and_paths_are_equivalent() {
        let semantics = WindowsPathSemantics;
        assert!(semantics.paths_equivalent(
            Path::new("MyMod/Config.json"),
            Path::new("mymod/CONFIG.JSON")
        ));
        let posix = PosixPathSemantics;
        assert!(!posix.paths_equivalent(
            Path::new("MyMod/Config.json"),
            Path::new("mymod/CONFIG.JSON")
        ));
    }

    #[test]
    fn windows_verbatim_prefixes_denote_the_same_path() {
        let semantics = WindowsPathSemantics;
        // A canonicalized Windows root gains the verbatim prefix; the user's
        // typed path does not. Both name one installation.
        assert!(semantics.paths_equivalent(
            Path::new(r"\\?\C:\Games\Stardew"),
            Path::new(r"C:\Games\Stardew")
        ));
        // The verbatim UNC form is the ordinary UNC share without the two
        // leading separators that Win32 folds into the prefix.
        assert!(semantics.paths_equivalent(
            Path::new(r"\\?\UNC\server\share"),
            Path::new(r"\\server\share")
        ));
        assert!(semantics.paths_equivalent(
            Path::new(r"\\?\unc\server\share"),
            Path::new(r"\\SERVER\SHARE")
        ));
        // Mixed separators fold the same way through the prefix.
        assert!(semantics.paths_equivalent(
            Path::new(r"\\?\C:/Games/Stardew"),
            Path::new("c:/games/stardew")
        ));
        // Stripping the prefix must not make unrelated paths equivalent.
        assert!(!semantics.paths_equivalent(
            Path::new(r"\\?\C:\Games\Stardew"),
            Path::new(r"\\?\C:\Games\Stardew Valley")
        ));
    }

    #[test]
    fn posix_accepts_names_windows_rejects() {
        let posix = PosixPathSemantics;
        for name in ["CON", "config.json ", "config.json.", "a:b", "what?"] {
            assert!(
                posix.validate_component(name).is_ok(),
                "{} is a legal POSIX file name",
                name
            );
        }
        assert_eq!(posix.validate_component(""), Err(PathRuleViolation::Empty));
    }

    #[test]
    fn the_host_semantics_match_the_compiled_target() {
        let host = host_path_semantics();
        if cfg!(target_os = "windows") {
            assert_eq!(host.id(), "windows");
            assert!(host.is_case_insensitive());
        } else {
            assert_eq!(host.id(), "posix");
            assert!(!host.is_case_insensitive());
        }
    }
}
