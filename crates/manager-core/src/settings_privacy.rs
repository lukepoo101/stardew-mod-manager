//! Warnings about what a mod's settings file might reveal if shared, and
//! taking those fields out before sharing.
//!
//! Settings are usually harmless, but some mods store API keys, account names
//! or paths from the user's computer. This looks for the common shapes and
//! says what it found; it cannot prove a file is safe to share.
//!
//! Redaction edits the file's own text: only the fields taken out change, so
//! comments, key order and number formatting stay as the mod wrote them, and
//! the collection flow (`shared/recipe/settingsPrivacy.ts`) writes exactly the
//! same bytes for the same file.

use serde_json::Value;

const SECRET_WORDS: &[&str] = &[
    "apikey", "api_key", "token", "password", "passwd", "secret", "auth", "cookie", "session",
];
/// A field is never shared when its name (lower case, without `-`, `_` or
/// spaces) is one of these or ends with one: "ApiToken", "AccountPassword".
/// "ShowSecretNotes", "SecretWoods" and "MaxTokens" are ordinary settings.
/// The same list as the collection flow's.
const BLOCKED_ENDINGS: &[&str] = &[
    "apikey",
    "authkey",
    "accesskey",
    "privatekey",
    "token",
    "password",
    "passwd",
    "secret",
];
/// Endings that also name key bindings ("QuickAccessKey": "F3"), so their
/// text is blocked only when it looks like a credential, not a button.
const KEY_ENDINGS: &[&str] = &["authkey", "accesskey"];

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

/// Text that looks like a credential rather than a button name: at least
/// eight characters with no spaces or `+`, and a digit, `_` or `-` in it, or
/// at least twenty characters. "F3", "LeftShoulder" and "LeftShift + F3"
/// are buttons; "ak_secret3" is not.
fn looks_like_credential(text: &str) -> bool {
    let t = text.trim();
    let length = t.chars().count();
    length >= 8
        && !t.contains(|c: char| c.is_ascii_whitespace() || c == '+')
        && (length >= 20 || t.contains(|c: char| c.is_ascii_digit() || c == '_' || c == '-'))
}

/// A settings file read as a tree of positions in its own text.
enum Node {
    Object {
        open: usize,
        close: usize,
        members: Vec<Member>,
    },
    Array(Vec<Item>),
    Text(String),
    Number,
    Bool,
    Null,
}

struct Member {
    key: String,
    /// Where its key's opening quote is.
    start: usize,
    /// Just after its value.
    end: usize,
    value: Node,
}

struct Item {
    start: usize,
    end: usize,
    value: Node,
}

/// Reads JSON as SMAPI accepts it (comments, trailing commas, a byte-order
/// mark), keeping where each value is. Positions are byte offsets.
struct Scan<'a> {
    text: &'a str,
    i: usize,
}

impl Scan<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.i).copied()
    }

    fn space(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.i += 1,
                Some(b'/') if self.text[self.i..].starts_with("//") => {
                    self.i = self.text[self.i..]
                        .find('\n')
                        .map_or(self.text.len(), |n| self.i + n);
                }
                Some(b'/') if self.text[self.i..].starts_with("/*") => {
                    self.i = self.text[self.i + 2..]
                        .find("*/")
                        .map_or(self.text.len(), |n| self.i + 2 + n + 2);
                }
                _ => return,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        let start = self.i;
        self.i += 1;
        loop {
            match self.peek()? {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    break;
                }
                _ => self.i += 1,
            }
        }
        serde_json::from_str(self.text.get(start..self.i)?).ok()
    }

    fn value(&mut self) -> Option<Node> {
        match self.peek()? {
            b'{' => {
                let open = self.i;
                self.i += 1;
                let mut members = Vec::new();
                loop {
                    self.space();
                    if self.peek()? == b'}' {
                        break;
                    }
                    if self.peek()? != b'"' {
                        return None;
                    }
                    let start = self.i;
                    let key = self.string()?;
                    self.space();
                    if self.peek()? != b':' {
                        return None;
                    }
                    self.i += 1;
                    self.space();
                    let value = self.value()?;
                    members.push(Member {
                        key,
                        start,
                        end: self.i,
                        value,
                    });
                    self.space();
                    match self.peek()? {
                        b',' => self.i += 1,
                        b'}' => break,
                        _ => return None,
                    }
                }
                let close = self.i;
                self.i += 1;
                Some(Node::Object {
                    open,
                    close,
                    members,
                })
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.space();
                    if self.peek()? == b']' {
                        break;
                    }
                    let start = self.i;
                    let value = self.value()?;
                    items.push(Item {
                        start,
                        end: self.i,
                        value,
                    });
                    self.space();
                    match self.peek()? {
                        b',' => self.i += 1,
                        b']' => break,
                        _ => return None,
                    }
                }
                self.i += 1;
                Some(Node::Array(items))
            }
            b'"' => self.string().map(Node::Text),
            _ => {
                for (word, node) in [
                    ("true", Node::Bool),
                    ("false", Node::Bool),
                    ("null", Node::Null),
                ] {
                    if self.text[self.i..].starts_with(word) {
                        self.i += word.len();
                        return Some(node);
                    }
                }
                let start = self.i;
                while matches!(
                    self.peek(),
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    self.i += 1;
                }
                serde_json::from_str::<serde_json::Number>(&self.text[start..self.i])
                    .ok()
                    .map(|_| Node::Number)
            }
        }
    }
}

fn read_tree(text: &str) -> Option<Node> {
    let mut scan = Scan {
        text,
        i: if text.starts_with('\u{feff}') { 3 } else { 0 },
    };
    scan.space();
    let node = scan.value()?;
    scan.space();
    (scan.i == text.len()).then_some(node)
}

/// Whether a field named `key` (normalised) holding `value` is never shared.
/// True or false is never a secret, nor is nothing.
fn never_shared(key: &str, value: &Node) -> bool {
    if !BLOCKED_ENDINGS.iter().any(|w| key.ends_with(w)) {
        return false;
    }
    let key_like = KEY_ENDINGS.iter().any(|w| key.ends_with(w));
    fn holds(value: &Node, key_like: bool) -> bool {
        match value {
            Node::Text(t) if key_like => looks_like_credential(t),
            Node::Text(t) => !t.trim().is_empty(),
            Node::Number => !key_like,
            Node::Bool | Node::Null => false,
            Node::Object { members, .. } => members.iter().any(|m| holds(&m.value, key_like)),
            Node::Array(items) => items.iter().any(|i| holds(&i.value, key_like)),
        }
    }
    holds(value, key_like)
}

struct Found {
    path: String,
    blocked: bool,
}

/// Walks the tree, naming each field worth taking out and, for the ones
/// that will be, the text to change: `(from, to, replacement)`.
fn walk_tree(
    node: &Node,
    path: &str,
    keep_flagged: bool,
    found: &mut Vec<Found>,
    edits: &mut Vec<(usize, usize, &'static str)>,
) {
    match node {
        Node::Object {
            open,
            close,
            members,
        } => {
            let mut gone = Vec::new();
            for (index, member) in members.iter().enumerate() {
                let here = if path.is_empty() {
                    member.key.clone()
                } else {
                    format!("{path}.{}", member.key)
                };
                let k = normalise(&member.key);
                if never_shared(&k, &member.value) {
                    found.push(Found {
                        path: here,
                        blocked: true,
                    });
                    gone.push(index);
                    continue;
                }
                let secretish = SECRET_WORDS.iter().any(|w| k.contains(&normalise(w)));
                if secretish && matches!(&member.value, Node::Text(t) if !t.trim().is_empty()) {
                    found.push(Found {
                        path: here,
                        blocked: false,
                    });
                    if !keep_flagged {
                        gone.push(index);
                    }
                    continue;
                }
                if let Node::Text(t) = &member.value {
                    if text_is_private(t) {
                        found.push(Found {
                            path: here,
                            blocked: false,
                        });
                        if !keep_flagged {
                            gone.push(index);
                        }
                    }
                    continue;
                }
                walk_tree(&member.value, &here, keep_flagged, found, edits);
            }
            if gone.is_empty() {
                return;
            }
            if gone.len() == members.len() {
                edits.push((open + 1, *close, ""));
                return;
            }
            // Each run of fields taken out goes with the separator after it,
            // or, at the end, the one before it.
            let mut run = 0;
            while run < gone.len() {
                let first = gone[run];
                let mut last = first;
                while run + 1 < gone.len() && gone[run + 1] == last + 1 {
                    run += 1;
                    last += 1;
                }
                if last + 1 < members.len() {
                    edits.push((members[first].start, members[last + 1].start, ""));
                } else {
                    edits.push((members[first - 1].end, members[last].end, ""));
                }
                run += 1;
            }
        }
        Node::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let here = format!("{path}[{index}]");
                if let Node::Text(t) = &item.value {
                    if text_is_private(t) {
                        found.push(Found {
                            path: here,
                            blocked: false,
                        });
                        if !keep_flagged {
                            edits.push((item.start, item.end, "\"\""));
                        }
                    }
                    continue;
                }
                walk_tree(&item.value, &here, keep_flagged, found, edits);
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
/// `keep_flagged`, as the collection flow does. Only those fields change;
/// a file with nothing taken out is returned byte for byte. Text that is not
/// JSON is returned as it is; [`json_problem`] says why it cannot be shared.
pub fn redact_settings(bytes: &[u8], keep_flagged: bool) -> Redaction {
    let unchanged = || Redaction {
        bytes: bytes.to_vec(),
        blocked: Vec::new(),
        removed: Vec::new(),
    };
    let Some(text) = std::str::from_utf8(bytes).ok() else {
        return unchanged();
    };
    let Some(tree) = read_tree(text) else {
        return unchanged();
    };
    let mut found = Vec::new();
    let mut edits = Vec::new();
    walk_tree(&tree, "", keep_flagged, &mut found, &mut edits);
    let mut blocked = Vec::new();
    let mut removed = Vec::new();
    for f in found {
        let list = if f.blocked {
            &mut blocked
        } else if keep_flagged {
            continue;
        } else {
            &mut removed
        };
        if !list.contains(&f.path) {
            list.push(f.path);
        }
    }
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = text.to_string();
    for (from, to, replacement) in edits {
        out.replace_range(from..to, replacement);
    }
    Redaction {
        bytes: out.into_bytes(),
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

    /// Cases shared word for word with the collection flow's tests
    /// (`SettingsPrivacy.test.ts`), so both write the same bytes.
    /// (file, keep flagged, shared text, never shared, left out)
    pub(crate) const SHARED_CASES: &[(&str, bool, &str, &[&str], &[&str])] = &[
        // Ordinary settings that only contain a secret-like word.
        (r#"{"ShowSecretNotes": true, "Volume": 3}"#, true, r#"{"ShowSecretNotes": true, "Volume": 3}"#, &[], &[]),
        (r#"{"ShowSecretNotes": true, "Volume": 3}"#, false, r#"{"ShowSecretNotes": true, "Volume": 3}"#, &[], &[]),
        (r#"{"SecretWoods": {"Enabled": true}, "Volume": 3}"#, true, r#"{"SecretWoods": {"Enabled": true}, "Volume": 3}"#, &[], &[]),
        (r#"{"SecretWoods": {"Enabled": true}, "Volume": 3}"#, false, r#"{"SecretWoods": {"Enabled": true}, "Volume": 3}"#, &[], &[]),
        (r#"{"MaxTokens": 5, "Volume": 3}"#, true, r#"{"MaxTokens": 5, "Volume": 3}"#, &[], &[]),
        (r#"{"MaxTokens": 5, "Volume": 3}"#, false, r#"{"MaxTokens": 5, "Volume": 3}"#, &[], &[]),
        (r#"{"Controls": {"QuickAccessKey": "F3"}}"#, true, r#"{"Controls": {"QuickAccessKey": "F3"}}"#, &[], &[]),
        (r#"{"Controls": {"QuickAccessKey": "F3"}}"#, false, r#"{"Controls": {"QuickAccessKey": "F3"}}"#, &[], &[]),
        (r#"{"ToggleKey": "F5", "Session": "x"}"#, false, r#"{"ToggleKey": "F5"}"#, &[], &["Session"]),
        // Secrets that never travel.
        (r#"{"ApiToken": "ghp_secret1", "ApiToken": ""}"#, false, r#"{"ApiToken": ""}"#, &["ApiToken"], &[]),
        (r#"{"ApiToken": {"Value": "ghp_secret2"}, "Volume": 1}"#, true, r#"{"Volume": 1}"#, &["ApiToken"], &[]),
        (r#"{"Password": 123456}"#, true, "{}", &["Password"], &[]),
        (r#"{"AuthKey": "ak_secret3", "Path": "/home/sam/x"}"#, true, r#"{"Path": "/home/sam/x"}"#, &["AuthKey"], &[]),
        (r#"{"AuthKey": "ak_secret3", "Path": "/home/sam/x"}"#, false, "{}", &["AuthKey"], &["Path"]),
        (r#"{"Recent": ["/home/sam/a", "ok"]}"#, false, r#"{"Recent": ["", "ok"]}"#, &[], &["Recent[0]"]),
        // Comments, key order and number formatting are left as they are.
        (
            "{\n  // Cloud sync\n  \"Zoom\": 1,\n  \"ApiToken\": \"ghp_abc\",\n  \"Alpha\": 2.0,\n  \"SaveBackupFolder\": \"/home/luke/Backups\"\n}\n",
            false,
            "{\n  // Cloud sync\n  \"Zoom\": 1,\n  \"Alpha\": 2.0\n}\n",
            &["ApiToken"],
            &["SaveBackupFolder"],
        ),
    ];

    #[test]
    fn only_secrets_and_chosen_flags_are_taken_out_leaving_the_rest_of_the_text() {
        for (file, keep, shared, blocked, removed) in SHARED_CASES {
            let out = redact_settings(file.as_bytes(), *keep);
            assert_eq!(
                String::from_utf8(out.bytes).unwrap(),
                *shared,
                "{file} keep={keep}"
            );
            assert_eq!(out.blocked, *blocked, "{file}");
            assert_eq!(out.removed, *removed, "{file}");
        }
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
