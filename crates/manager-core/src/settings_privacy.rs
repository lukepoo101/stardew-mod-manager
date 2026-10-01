//! Warnings about what a mod's settings file might reveal if shared.
//!
//! Settings are usually harmless, but some mods store API keys, account names
//! or paths from the user's computer. This looks for the common shapes and
//! says what it found; it cannot prove a file is safe to share.

use serde_json::Value;

const SECRET_WORDS: &[&str] = &[
    "apikey", "api_key", "token", "password", "passwd", "secret", "auth", "cookie", "session",
];

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
    fn broken_json_is_still_checked_as_text() {
        let warnings = settings_warnings(b"not json C:\\Users\\sam\\file");
        assert_eq!(
            warnings,
            vec!["The file contains a path from this computer".to_string()]
        );
    }
}
