//! A tolerant reader for Valve's KeyValues text format.
//!
//! Steam writes libraryfolders.vdf with the same syntax on every platform, so
//! this parser is deliberately not platform-specific. It is tolerant by design:
//! a corrupt or partially written file has to produce "no libraries found"
//! rather than a hard failure, because the manager still has manual folder
//! selection as a fallback and must not refuse to start.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// One parsed KeyValues object: a mapping of key to either a scalar or a child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdfValue {
    Scalar(String),
    Object(VdfObject),
}

pub type VdfObject = BTreeMap<String, VdfValue>;

impl VdfValue {
    pub fn as_scalar(&self) -> Option<&str> {
        match self {
            Self::Scalar(value) => Some(value.as_str()),
            Self::Object(_) => None,
        }
    }

    pub fn as_object(&self) -> Option<&VdfObject> {
        match self {
            Self::Object(object) => Some(object),
            Self::Scalar(_) => None,
        }
    }

    /// Case-insensitive scalar lookup, which is how Valve's own loader behaves.
    pub fn get_scalar(&self, key: &str) -> Option<&str> {
        self.as_object()?
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .and_then(|(_, value)| value.as_scalar())
    }
}

/// Parses a KeyValues document into its top-level object.
pub fn parse_vdf(content: &str) -> VdfObject {
    let tokens = tokenize(content);
    let mut cursor = 0usize;
    parse_object(&tokens, &mut cursor)
}

fn tokenize(content: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut escaped = false;

    for character in strip_line_comments(content).chars() {
        if escaped {
            // Valve escapes backslashes and quotes; anything else stands for
            // itself so a stray backslash cannot swallow the rest of a path.
            current.push(match character {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
            escaped = false;
            continue;
        }

        if in_quote {
            match character {
                '\\' => escaped = true,
                '"' => {
                    tokens.push(std::mem::take(&mut current));
                    in_quote = false;
                }
                other => current.push(other),
            }
            continue;
        }

        match character {
            '"' => in_quote = true,
            '{' | '}' => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
                tokens.push(character.to_string());
            }
            other if other.is_whitespace() => {
                if !current.trim().is_empty() {
                    tokens.push(current.trim().to_string());
                }
                current.clear();
            }
            other => current.push(other),
        }
    }

    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }
    tokens
}

/// Removes // line comments that appear outside quoted strings.
fn strip_line_comments(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut in_quote = false;
    let mut escaped = false;
    let mut characters = content.chars().peekable();

    while let Some(character) = characters.next() {
        if escaped {
            output.push(character);
            escaped = false;
            continue;
        }
        if in_quote {
            output.push(character);
            if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_quote = false;
            }
            continue;
        }
        if character == '"' {
            in_quote = true;
            output.push(character);
            continue;
        }
        if character == '/' && characters.peek() == Some(&'/') {
            for next in characters.by_ref() {
                if next == '\n' {
                    output.push('\n');
                    break;
                }
            }
            continue;
        }
        output.push(character);
    }

    output
}

fn parse_object(tokens: &[String], cursor: &mut usize) -> VdfObject {
    let mut object = VdfObject::new();

    while *cursor < tokens.len() {
        let token = &tokens[*cursor];

        if token == "}" {
            *cursor += 1;
            return object;
        }
        if token == "{" {
            // Unbalanced opener: nothing sensible to attach it to.
            *cursor += 1;
            continue;
        }

        let key = token.clone();
        *cursor += 1;

        if *cursor >= tokens.len() {
            object.insert(key, VdfValue::Scalar(String::new()));
            return object;
        }

        match tokens[*cursor].as_str() {
            "{" => {
                *cursor += 1;
                let child = parse_object(tokens, cursor);
                object.insert(key, VdfValue::Object(child));
            }
            "}" => {
                object.insert(key, VdfValue::Scalar(String::new()));
                *cursor += 1;
                return object;
            }
            scalar => {
                object.insert(key, VdfValue::Scalar(scalar.to_string()));
                *cursor += 1;
            }
        }
    }

    object
}

/// Every Steam library root recorded in a libraryfolders.vdf document.
///
/// Steam has shipped two shapes of this file. The modern format wraps the
/// libraries in a `libraryfolders` object that nests one object per library
/// under a numbered key with a `path` member. The legacy format puts the path
/// directly against a numeric or `BaseInstallFolder_` key.
///
/// Both are supported, and so is a document that is not wrapped at all, which
/// is what a hand-edited or truncated file looks like. The reader therefore
/// collects path members from every object it finds rather than assuming one
/// particular shape, because a library that is silently skipped is a game the
/// user cannot find.
pub fn library_paths_from_vdf(content: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for object in library_objects(&parse_vdf(content)) {
        collect_paths_from(object, &mut paths);
    }
    paths
}

/// The objects that may hold library entries.
fn library_objects(root: &VdfObject) -> Vec<&VdfObject> {
    let named = root
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("libraryfolders"))
        .and_then(|(_, value)| value.as_object());

    let mut objects = Vec::new();
    match named {
        Some(inner) => objects.push(inner),
        // An unwrapped document is read as if it were the container.
        None => objects.push(root),
    }
    objects
}

fn collect_paths_from(object: &VdfObject, paths: &mut Vec<PathBuf>) {
    for (key, value) in object {
        match value {
            VdfValue::Object(_) => {
                if let Some(path) = value.get_scalar("path") {
                    push_unique(paths, path);
                }
            }
            VdfValue::Scalar(scalar) => {
                let is_path_key = key.eq_ignore_ascii_case("path")
                    || key.to_ascii_lowercase().starts_with("baseinstallfolder_")
                    || key.chars().all(|c| c.is_ascii_digit());
                if is_path_key {
                    push_unique(paths, scalar);
                }
            }
        }
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    let candidate = PathBuf::from(normalize_vdf_path(trimmed));
    if !paths.contains(&candidate) {
        paths.push(candidate);
    }
}

/// The path as written in the VDF file.
///
/// Escaping is resolved during tokenization, so no separator rewriting happens
/// here: Steam writes "D:\\SteamLibrary" on Windows and "/home/user/.steam/root"
/// on Linux, and on Windows both '/' and '\\' are separators as far as the
/// filesystem APIs are concerned. Rewriting them would corrupt a path that
/// already uses the host's own convention.
pub fn normalize_vdf_path(value: &str) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_library_folders_are_read_with_labels() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/user/.local/share/Steam"
		"label"		""
		"apps"
		{
			"413150"	"1234567890"
		}
	}
	"1"
	{
		"path"		"/mnt/games/SteamLibrary"
		"label"		"SSD Games"
	}
}
"#;
        let paths = library_paths_from_vdf(vdf);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/home/user/.local/share/Steam"));
        assert_eq!(paths[1], PathBuf::from("/mnt/games/SteamLibrary"));
    }

    #[test]
    fn legacy_library_folders_are_read() {
        let vdf = r#"
"LibraryFolders"
{
	"TimeNextStatsReport"		"123456789"
	"ContentStatsID"		"987654321"
	"1"		"/media/secondary/Steam"
	"BaseInstallFolder_2"		"/mnt/storage/SteamLibrary"
}
"#;
        let paths = library_paths_from_vdf(vdf);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/media/secondary/Steam"));
        assert_eq!(paths[1], PathBuf::from("/mnt/storage/SteamLibrary"));
    }

    #[test]
    fn windows_escaped_paths_survive_tokenization() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"E:\\\\Games\\\\Steam\"\n\t}\n}\n";
        let paths = library_paths_from_vdf(vdf);
        assert_eq!(paths.len(), 2);
        let rendered: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(rendered, vec!["D:/SteamLibrary", "E:/Games/Steam"]);
    }

    #[test]
    fn malformed_documents_do_not_panic_or_invent_paths() {
        for vdf in [
            "",
            "not a vdf at all",
            "\"libraryfolders\" { \"path\" }",
            "\"libraryfolders\" { \"0\" { \"path\" \"\" } }",
        ] {
            let paths = library_paths_from_vdf(vdf);
            assert!(
                paths.iter().all(|path| !path.as_os_str().is_empty()),
                "empty paths must never be reported: {vdf:?}"
            );
        }
    }

    #[test]
    fn apps_manifest_keys_are_available_for_validation() {
        let manifest = r#"
"AppState"
{
	"appid"		"413150"
	"installdir"		"Stardew Valley"
	"name"		"Stardew Valley"
}
"#;
        let root = parse_vdf(manifest);
        let app_state = root.get("AppState").expect("AppState object");
        assert_eq!(app_state.get_scalar("appid"), Some("413150"));
        assert_eq!(app_state.get_scalar("installdir"), Some("Stardew Valley"));
    }
}
