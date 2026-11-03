//! Warnings about what a mod's settings file might reveal if shared.
//!
//! Settings are usually harmless, but some mods store API keys, account names
//! or paths from the user's computer. This looks for the common shapes and
//! says what it found; it cannot prove a file is safe to share.

use serde_json::Value;

const SECRET_WORDS: &[&str] = &[
    "apikey", "api_key", "token", "password", "passwd", "secret", "auth", "cookie", "session",
];
/// Never shared, whatever the person sharing chooses. The same list as the
/// collection flow's (`shared/recipe/settingsPrivacy.ts`).
const BLOCKED_WORDS: &[&str] = &["apikey", "token", "password", "passwd", "secret"];

/// Reads settings text the way SMAPI does: a byte-order mark, comments and
/// trailing commas are allowed.
fn parse_settings(bytes: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text)
        .or_else(|_| serde_json::from_str(&crate::manifest::parser::clean_json_comments(text)))
        .ok()
}

/// Why a settings file is not usable as shared settings, if it is not: it
/// must be text holding a JSON object, as a mod's `config.json` does.
pub fn json_problem(bytes: &[u8]) -> Option<String> {
    match parse_settings(bytes) {
        Some(Value::Object(_)) => None,
        Some(_) => Some("is not a settings object".to_string()),
        None => Some("is not JSON".to_string()),
    }
}

fn normalise(key: &str) -> String {
    key.to_lowercase().replace(['-', '_', ' '], "")
}

fn text_is_private(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("/home/")
        || lower.contains("/users/")
        || lower.contains("c:\\users\\")
        || lower.contains("c:/users/")
        || text
            .split_whitespace()
            .any(|word| word.contains('@') && word.contains('.') && !word.starts_with('@'))
}

/// Fields worth taking out, as (path, always blocked).
fn flagged(value: &Value, path: &str, out: &mut Vec<(String, bool)>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let here = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                let k = normalise(key);
                let secretish = SECRET_WORDS.iter().any(|w| k.contains(&normalise(w)));
                if secretish && matches!(inner, Value::String(s) if !s.trim().is_empty()) {
                    let blocked = BLOCKED_WORDS.iter().any(|w| k.contains(w));
                    out.push((here, blocked));
                    continue;
                }
                flagged(inner, &here, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flagged(item, &format!("{path}[{index}]"), out);
            }
        }
        Value::String(text) if text_is_private(text) => out.push((path.to_string(), false)),
        _ => {}
    }
}

fn remove(value: &mut Value, paths: &[String], path: &str) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| {
                let here = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                !paths.contains(&here)
            });
            for (key, inner) in map.iter_mut() {
                let here = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                remove(inner, paths, &here);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter_mut().enumerate() {
                let here = format!("{path}[{index}]");
                if paths.contains(&here) {
                    *item = Value::String(String::new());
                } else {
                    remove(item, paths, &here);
                }
            }
        }
        _ => {}
    }
}

/// A settings file as it will be shared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    pub bytes: Vec<u8>,
    /// Fields taken out because they are never shared (keys, tokens,
    /// passwords).
    pub blocked: Vec<String>,
    /// Flagged fields (paths, email addresses, account details) taken out
    /// because the person sharing did not choose to keep them.
    pub removed: Vec<String>,
}

/// Takes out fields that are never shared, and other flagged fields unless
/// `keep_flagged`, as the collection flow does. Text that is not JSON is
/// returned as it is; [`json_problem`] says why it cannot be shared.
pub fn redact_settings(bytes: &[u8], keep_flagged: bool) -> Redaction {
    let unchanged = || Redaction {
        bytes: bytes.to_vec(),
        blocked: Vec::new(),
        removed: Vec::new(),
    };
    let Some(mut value) = parse_settings(bytes) else {
        return unchanged();
    };
    let mut flags = Vec::new();
    flagged(&value, "", &mut flags);
    let blocked: Vec<String> = flags
        .iter()
        .filter(|(_, b)| *b)
        .map(|(p, _)| p.clone())
        .collect();
    let removed: Vec<String> = if keep_flagged {
        Vec::new()
    } else {
        flags
            .iter()
            .filter(|(_, b)| !*b)
            .map(|(p, _)| p.clone())
            .collect()
    };
    if blocked.is_empty() && removed.is_empty() {
        return unchanged();
    }
    let gone: Vec<String> = blocked.iter().chain(&removed).cloned().collect();
    remove(&mut value, &gone, "");
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    Redaction {
        bytes: format!("{text}\n").into_bytes(),
        blocked,
        removed,
    }
}

fn check_text(text: &str, where_: &str, out: &mut Vec<String>) {
    let lower = text.to_lowercase();
    if lower.contains("/home/")
        || lower.contains("/users/")
        || lower.contains("c:\\\\users\\\\")
        || lower.contains("c:\\users\\")
        || lower.contains("c:/users/")
    {
        out.push(format!("{where_} contains a path from this computer"));
    }
    if text.contains('@')
        && text
            .split_whitespace()
            .any(|word| word.contains('@') && word.contains('.') && !word.starts_with('@'))
    {
        out.push(format!("{where_} looks like it contains an email address"));
    }
}

fn walk(value: &Value, path: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let here = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                let key_lower = key.to_lowercase().replace(['-', ' '], "_");
                let secretish = SECRET_WORDS
                    .iter()
                    .any(|word| key_lower.replace('_', "").contains(&word.replace('_', "")));
                let has_value = matches!(inner, Value::String(s) if !s.trim().is_empty());
                if secretish && has_value {
                    out.push(format!("\"{here}\" may be a key, token or password"));
                }
                walk(inner, &here, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(item, &format!("{path}[{index}]"), out);
            }
        }
        Value::String(text) => check_text(text, &format!("\"{path}\""), out),
        _ => {}
    }
}

/// Plain-language warnings for one settings file. Unreadable JSON is checked
/// as text.
pub fn settings_warnings(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let text = String::from_utf8_lossy(bytes);
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => walk(&value, "", &mut out),
        Err(_) => check_text(&text, "The file", &mut out),
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_settings_are_quiet() {
        assert!(settings_warnings(br#"{"Volume": 3, "ShowHud": true, "Key": "F5"}"#).is_empty());
    }

    #[test]
    fn secrets_paths_and_emails_are_named() {
        let warnings = settings_warnings(
            br#"{"Api": {"ApiKey": "abc123"}, "LogFolder": "/home/sam/logs", "Contact": "sam@example.com", "Token": ""}"#,
        );
        assert!(warnings.iter().any(|w| w.contains("Api.ApiKey")));
        assert!(warnings
            .iter()
            .any(|w| w.contains("path from this computer")));
        assert!(warnings.iter().any(|w| w.contains("email")));
        // An empty token is not a secret.
        assert!(!warnings.iter().any(|w| w.contains("\"Token\"")));
    }

    #[test]
    fn secrets_are_always_taken_out_and_other_flags_unless_kept() {
        let file = br#"{"ApiToken": "ghp_abc", "Backup": "/home/sam/saves", "Mail": "sam@example.com", "Volume": 3, "Session": "x"}"#;
        let shared = redact_settings(file, false);
        let text = String::from_utf8(shared.bytes.clone()).unwrap();
        assert!(!text.contains("ghp_abc") && !text.contains("/home/sam"));
        assert!(!text.contains("sam@example.com") && text.contains("Volume"));
        assert_eq!(shared.blocked, vec!["ApiToken".to_string()]);
        assert!(shared.removed.contains(&"Backup".to_string()));
        assert!(shared.removed.contains(&"Session".to_string()));
        assert!(settings_warnings(&shared.bytes).is_empty(), "{text}");

        // Keeping flagged fields still never keeps a token.
        let kept = redact_settings(file, true);
        let text = String::from_utf8(kept.bytes).unwrap();
        assert!(!text.contains("ghp_abc") && text.contains("/home/sam"));
        assert!(kept.removed.is_empty());

        // Nothing flagged: the bytes are shared exactly as they are.
        let plain = b"{\n  // comment\n  \"Volume\": 3,\n}";
        assert_eq!(redact_settings(plain, false).bytes, plain.to_vec());
    }

    #[test]
    fn settings_must_be_a_json_object() {
        assert!(json_problem(b"{\"a\": 1}").is_none());
        assert!(json_problem(b"\xef\xbb\xbf{ /* SMAPI allows this */ \"a\": 1, }").is_none());
        assert_eq!(json_problem(b"not json {").as_deref(), Some("is not JSON"));
        assert!(json_problem(b"[1, 2]").is_some());
        assert!(json_problem(&[0xff, 0xfe]).is_some());
    }

    #[test]
    fn broken_json_is_still_checked_as_text() {
        let warnings = settings_warnings(b"not json C:\\Users\\sam\\file");
        assert_eq!(
            warnings,
            vec!["The file contains a path from this computer".to_string()]
        );
    }
}
