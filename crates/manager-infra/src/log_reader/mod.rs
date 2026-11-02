use chrono::{DateTime, Utc};
use manager_app::error::{AppError, AppResult};
use manager_app::ports::logging::{ExpectedMod, SessionLogPort};
use manager_core::launch::{
    LoadOutcome, ModVerificationEvidence, SessionSkippedMod, SessionVerificationBaseline,
    SessionVerificationResult,
};
use manager_core::path_semantics::host_path_semantics;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmapiLogPrefix<'a> {
    pub time_str: &'a str,
    pub level: &'a str,
    pub source: &'a str,
}

pub fn strip_smapi_prefix(line: &str) -> (&str, Option<SmapiLogPrefix<'_>>) {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('[') {
        return (line, None);
    }
    if let Some(close_bracket) = trimmed.find(']') {
        let inside = &trimmed[1..close_bracket];
        let mut parts = inside.splitn(3, ' ');
        if let (Some(time_str), Some(level)) = (parts.next(), parts.next()) {
            if time_str.len() == 8
                && time_str.as_bytes()[2] == b':'
                && time_str.as_bytes()[5] == b':'
            {
                let source = parts.next().unwrap_or("").trim();
                let raw_msg = &trimmed[close_bracket + 1..];
                let msg = raw_msg.strip_prefix(' ').unwrap_or(raw_msg);
                return (
                    msg,
                    Some(SmapiLogPrefix {
                        time_str,
                        level: level.trim(),
                        source,
                    }),
                );
            }
        }
    }
    (line, None)
}

fn parse_smapi_date(date_str: &str) -> Option<DateTime<Utc>> {
    let cleaned = date_str.trim().trim_end_matches(" UTC").trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(cleaned) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(cleaned, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(cleaned, "%Y-%m-%dT%H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(cleaned, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(cleaned, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    None
}

/// How the Mods folder SMAPI reported compares with the one the session
/// asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ModsFolderEvidence {
    Matched,
    Different(String),
    /// Not in the log, or written in a form that cannot be compared.
    Unreadable,
}

fn compare_mods_folder(observed: Option<&str>, expected: Option<&Path>) -> ModsFolderEvidence {
    let (Some(observed), Some(expected)) = (observed, expected) else {
        return ModsFolderEvidence::Unreadable;
    };
    let observed_path = Path::new(observed);
    // Log sharing tools replace the user name with asterisks, and some write
    // ~ or %USERPROFILE%. Such a path says nothing about which folder it was.
    if observed.contains('*') || observed.contains('%') || !observed_path.is_absolute() {
        return ModsFolderEvidence::Unreadable;
    }
    // SMAPI prints the path in its own separators and casing, so the
    // comparison follows host path semantics rather than raw string equality:
    // C:\Users\Luke\... and c:/Users/Luke/... are the same directory.
    if host_path_semantics().paths_equivalent(observed_path, expected) {
        return ModsFolderEvidence::Matched;
    }
    // A symlinked data folder prints differently but is the same directory.
    match (observed_path.canonicalize(), expected.canonicalize()) {
        (Ok(a), Ok(b)) if a == b => ModsFolderEvidence::Matched,
        _ => ModsFolderEvidence::Different(observed.to_string()),
    }
}

/// The display name and UniqueID in one of SMAPI's per-mod TRACE lines.
///
/// SMAPI 4 writes these while loading mods (format strings " (from ",
/// ", ID: ", ", assembly version: " and ") [content pack]..." in
/// StardewModdingAPI.dll):
///
/// ```text
///    Name (from Mods/X/X.dll, ID: Some.Id, assembly version: 1.0.0)...
///    Name (from Mods/X, ID: Some.Id) [content pack]...
/// ```
///
/// The shorter `Name (ID: Some.Id)` is accepted too.
pub fn parse_mod_identity(message: &str) -> Option<(String, String)> {
    let text = message.trim();
    let (name, ids) = if let Some(at) = text.find(" (from ") {
        let rest = &text[at + " (from ".len()..];
        let id_at = rest.find(", ID: ")?;
        (&text[..at], &rest[id_at + ", ID: ".len()..])
    } else {
        let at = text.find(" (ID: ")?;
        (&text[..at], &text[at + " (ID: ".len()..])
    };
    let id: String = ids
        .chars()
        .take_while(|c| *c != ',' && *c != ')' && !c.is_whitespace())
        .collect();
    let name = name.trim();
    (!name.is_empty() && !id.is_empty()).then(|| (name.to_string(), id))
}

/// What happened to one expected mod, according to the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModEvidence {
    /// In SMAPI's loaded list, and tied to its UniqueID.
    Confirmed,
    Skipped,
    /// In the loaded list by name, but not tied to its UniqueID.
    Unconfirmable,
    /// Neither loaded nor skipped.
    NotSeen,
}

fn names(list: &[String]) -> String {
    list.join(", ")
}

/// Reads one session's evidence out of SMAPI's log.
///
/// Nothing is concluded about the mods until SMAPI has printed its list of
/// loaded mods. After that, a mod counts as confirmed only when it is in that
/// list and SMAPI's loading trace ties the name to the mod's UniqueID. A log
/// that cannot show this (an anonymised or missing Mods path, no IDs) gives
/// "unavailable", never a failure, while mods SMAPI says it skipped are always
/// reported, with its reason.
pub fn evaluate_session_log(
    content: &str,
    baseline: &SessionVerificationBaseline,
    expected_mods: &[ExpectedMod],
    now: DateTime<Utc>,
) -> SessionVerificationResult {
    let mut observed_mods_path: Option<String> = None;
    let mut custom_mods_path_detected = false;
    let mut session_matched = false;
    let mut session_start_time = None;
    let mut start_line = 0;
    let mut load_finished = false;
    let mut loaded_entries: Vec<String> = Vec::new();
    let mut identities: Vec<(String, String)> = Vec::new();
    let mut in_loaded_section = false;

    for (index, line) in content.lines().enumerate() {
        let (msg, prefix) = strip_smapi_prefix(line);
        let Some(prefix) = prefix else {
            continue;
        };
        if prefix.source != "SMAPI" {
            continue;
        }
        let content = msg.trim();
        let indented = msg.starts_with("   ") || msg.starts_with('\t');

        // SMAPI writes these before "Log started at", so they are read from
        // the whole file; SMAPI-latest.txt holds one session.
        if let Some(path) = content.strip_prefix("Mods go here: ") {
            observed_mods_path = Some(path.trim().to_string());
        }
        if content.contains("--mods-path") {
            custom_mods_path_detected = true;
        }
        if let Some(pos) = content.find("Log started at ") {
            let parsed = parse_smapi_date(&content[pos + "Log started at ".len()..]);
            // Whole-second precision: the start may share the launch second.
            if let Some(parsed) =
                parsed.filter(|p| p.timestamp() >= baseline.launch_time.timestamp() && *p <= now)
            {
                session_matched = true;
                session_start_time = Some(parsed);
                start_line = index;
                load_finished = false;
                loaded_entries.clear();
                identities.clear();
                in_loaded_section = false;
            }
            continue;
        }
        if !session_matched {
            continue;
        }

        if !indented && !content.is_empty() {
            in_loaded_section = false;
        }
        // "Loaded 3 mods:", "Loaded 0 mods." and "Loaded 1 content packs:".
        if let Some(rest) = content.strip_prefix("Loaded ") {
            if rest.starts_with(|c: char| c.is_ascii_digit())
                && (rest.contains(" mods") || rest.contains(" content packs"))
            {
                load_finished = true;
                in_loaded_section = content.ends_with(':');
                continue;
            }
        }
        if in_loaded_section && indented && prefix.level == "INFO" {
            loaded_entries.push(content.to_string());
            continue;
        }
        if prefix.level == "TRACE" {
            if let Some(identity) = parse_mod_identity(content) {
                identities.push(identity);
            }
        }
    }

    if !session_matched {
        return SessionVerificationResult {
            custom_mods_path_detected,
            error_details: Some("SMAPI's log does not show this session yet.".to_string()),
            ..Default::default()
        };
    }

    // The skipped-mods block comes after the loaded lists; the summary parser
    // already reads SMAPI's shape of it.
    let session_log = content
        .lines()
        .skip(start_line)
        .collect::<Vec<_>>()
        .join("\n");
    let skipped = manager_core::smapi::log_summary::summarize_log(&session_log).skipped_mods;
    let skipped_mods: Vec<SessionSkippedMod> = skipped
        .iter()
        .map(|mod_| SessionSkippedMod {
            name: match &mod_.version {
                Some(version) => format!("{} {}", mod_.name, version),
                None => mod_.name.clone(),
            },
            reason: mod_.reason.clone(),
        })
        .collect();

    let folder = compare_mods_folder(
        observed_mods_path.as_deref(),
        baseline.expected_mods_path.as_deref(),
    );

    let mut verified_mods = Vec::new();
    let mut evidence = Vec::new();
    for expected in expected_mods {
        let was_skipped = skipped.iter().any(|s| {
            s.name.eq_ignore_ascii_case(&expected.name)
                && s.version.as_deref().is_none_or(|v| v == expected.version)
        });
        let label = format!("{} {}", expected.name, expected.version);
        let loaded_line = loaded_entries.iter().find(|entry| {
            entry.strip_prefix(&label).is_some_and(|rest| {
                rest.is_empty() || rest.starts_with(" by ") || rest.starts_with(" | ")
            })
        });
        // Two expected mods with the same name and version cannot be told
        // apart by SMAPI's loaded list.
        let unambiguous = expected_mods
            .iter()
            .filter(|other| other.name == expected.name && other.version == expected.version)
            .count()
            == 1;
        let has_identity = identities.iter().any(|(name, id)| {
            id.eq_ignore_ascii_case(expected.unique_id.as_str())
                && name.eq_ignore_ascii_case(&expected.name)
        });

        // A mod in the list of a folder that cannot be identified is not
        // confirmed: the list may belong to another folder.
        let state = if was_skipped {
            ModEvidence::Skipped
        } else if loaded_line.is_some()
            && unambiguous
            && has_identity
            && folder == ModsFolderEvidence::Matched
        {
            ModEvidence::Confirmed
        } else if loaded_line.is_some() {
            ModEvidence::Unconfirmable
        } else {
            ModEvidence::NotSeen
        };
        verified_mods.push(ModVerificationEvidence {
            unique_id: expected.unique_id.to_string(),
            expected_version: expected.version.clone(),
            found_in_log: state == ModEvidence::Confirmed,
            log_entry: loaded_line
                .filter(|_| state == ModEvidence::Confirmed)
                .cloned(),
        });
        evidence.push((expected.name.clone(), state));
    }

    let with = |wanted: ModEvidence| -> Vec<String> {
        evidence
            .iter()
            .filter(|(_, state)| *state == wanted)
            .map(|(name, _)| name.clone())
            .collect()
    };
    let not_seen = with(ModEvidence::NotSeen);
    let unconfirmable = with(ModEvidence::Unconfirmable);

    let (outcome, details) = if let ModsFolderEvidence::Different(observed) = &folder {
        (
            Some(LoadOutcome::WrongModsFolder),
            format!(
                "SMAPI used the Mods folder {observed}, not the one this session asked for ({}).",
                baseline
                    .expected_mods_path
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            ),
        )
    } else if !load_finished {
        (
            None,
            "SMAPI started, but its log does not list the mods it loaded yet.".to_string(),
        )
    } else {
        let folder_known = folder == ModsFolderEvidence::Matched;
        let mut parts = Vec::new();
        if !skipped_mods.is_empty() {
            parts.push(format!(
                "SMAPI skipped {}.",
                skipped_mods
                    .iter()
                    .map(|m| format!("{} because {}", m.name, m.reason.trim_end_matches('.')))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if folder_known && !not_seen.is_empty() {
            parts.push(format!(
                "Not in SMAPI's list of loaded mods: {}.",
                names(&not_seen)
            ));
        }
        if !folder_known {
            parts.push(
                "SMAPI's log does not show which Mods folder it used (the path is missing or anonymised), so its list of mods cannot be tied to this profile."
                    .to_string(),
            );
        } else if !unconfirmable.is_empty() {
            parts.push(format!(
                "SMAPI's log lists {} by name but not with a mod ID the manager can match, so they could not be confirmed.",
                names(&unconfirmable)
            ));
        }

        let outcome = if !skipped_mods.is_empty() || (folder_known && !not_seen.is_empty()) {
            LoadOutcome::Partial
        } else if !folder_known || !unconfirmable.is_empty() {
            LoadOutcome::Unavailable
        } else {
            LoadOutcome::Confirmed
        };
        if outcome == LoadOutcome::Confirmed {
            parts.push(if expected_mods.is_empty() {
                "SMAPI started with the Mods folder this session asked for.".to_string()
            } else {
                format!(
                    "SMAPI's log confirmed all {} expected mod(s) loaded from this profile's Mods folder.",
                    expected_mods.len()
                )
            });
        }
        (Some(outcome), parts.join(" "))
    };

    let mut unconfirmed_mods = not_seen;
    unconfirmed_mods.extend(unconfirmable);
    let all_mods_confirmed = outcome == Some(LoadOutcome::Confirmed);
    SessionVerificationResult {
        session_matched: true,
        session_start_time,
        custom_mods_path_detected,
        all_mods_confirmed,
        verified_mods,
        error_details: Some(details),
        outcome,
        skipped_mods,
        unconfirmed_mods,
    }
}

/// Where the reader looks for the SMAPI log.
///
/// The lookup is a platform capability rather than an environment conditional:
/// Linux uses ~/.config/StardewValley, Windows uses %APPDATA%\StardewValley, and
/// the parser below is shared by both.
pub trait SmapiLogLocator: Send + Sync {
    /// The log path this reader should use when no explicit path was supplied.
    fn log_path(&self) -> PathBuf;
}

/// The locator for the currently running host.
#[derive(Debug, Clone, Default)]
pub struct HostSmapiLogLocator;

impl SmapiLogLocator for HostSmapiLogLocator {
    fn log_path(&self) -> PathBuf {
        crate::platform::default_smapi_log_path(manager_core::game::OperatingSystem::host())
            .unwrap_or_else(|| PathBuf::from("SMAPI-latest.txt"))
    }
}

/// A locator that always reports the same path, for tests and explicit configuration.
#[derive(Debug, Clone)]
pub struct FixedSmapiLogLocator(pub PathBuf);

impl SmapiLogLocator for FixedSmapiLogLocator {
    fn log_path(&self) -> PathBuf {
        self.0.clone()
    }
}

pub struct SmapiSessionLogReader {
    locator: Box<dyn SmapiLogLocator>,
    custom_log_path: Option<PathBuf>,
}

impl SmapiSessionLogReader {
    /// A reader that resolves the log through the platform locator.
    pub fn new(custom_log_path: Option<PathBuf>) -> Self {
        Self {
            locator: Box::new(HostSmapiLogLocator),
            custom_log_path,
        }
    }

    /// A reader whose log location is supplied, which is how the platform
    /// locators are wired in and how tests pin a path.
    pub fn with_locator(
        locator: Box<dyn SmapiLogLocator>,
        custom_log_path: Option<PathBuf>,
    ) -> Self {
        Self {
            locator,
            custom_log_path,
        }
    }

    /// The log location this host would use.
    pub fn default_log_path() -> PathBuf {
        HostSmapiLogLocator.log_path()
    }

    pub fn log_path(&self) -> PathBuf {
        self.custom_log_path
            .clone()
            .unwrap_or_else(|| self.locator.log_path())
    }
}

impl SmapiSessionLogReader {
    fn capture_baseline_inner(&self) -> Result<SessionVerificationBaseline, String> {
        let path = self.log_path();
        let (initial_mtime, initial_size, initial_inode) = if path.exists() {
            let meta = std::fs::metadata(&path)
                .map_err(|e| format!("Failed to read log file metadata: {}", e))?;
            let mtime: DateTime<Utc> = meta
                .modified()
                .ok()
                .map(|t| t.into())
                .unwrap_or_else(Utc::now);
            #[cfg(unix)]
            let inode = {
                use std::os::unix::fs::MetadataExt;
                Some(meta.ino())
            };
            #[cfg(not(unix))]
            let inode = None;
            (Some(mtime), meta.len(), inode)
        } else {
            (None, 0, None)
        };

        Ok(SessionVerificationBaseline {
            log_path: path,
            initial_mtime,
            initial_size,
            launch_time: Utc::now(),
            initial_inode,
            expected_mods_path: None,
        })
    }

    fn verify_session_inner(
        &self,
        baseline: &SessionVerificationBaseline,
        expected_mods: &[ExpectedMod],
    ) -> Result<SessionVerificationResult, String> {
        let path = &baseline.log_path;
        if !path.exists() {
            return Ok(SessionVerificationResult {
                error_details: Some("Log file does not exist yet".to_string()),
                ..Default::default()
            });
        }

        let meta =
            std::fs::metadata(path).map_err(|e| format!("Failed to read log metadata: {}", e))?;

        if baseline
            .initial_mtime
            .is_some_and(|old| meta.modified().ok().map(DateTime::<Utc>::from) == Some(old))
            && meta.len() == baseline.initial_size
        {
            return Err("Log has not changed since launch".into());
        }
        let current_mtime: DateTime<Utc> = meta
            .modified()
            .ok()
            .map(|t| t.into())
            .unwrap_or_else(Utc::now);

        // Stale log check: mtime must be near or after launch (allow 2s clock skew)
        if current_mtime < baseline.launch_time - chrono::Duration::seconds(2) {
            return Ok(SessionVerificationResult {
                error_details: Some(
                    "Log file has not been modified since game launch (stale log)".to_string(),
                ),
                ..Default::default()
            });
        }

        // Always read from start to detect session header correctly across truncation or replacement
        let mut file = File::open(path)
            .map_err(|e| format!("Failed to open log file '{}': {}", path.display(), e))?;

        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Failed to seek in log file: {}", e))?;

        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| format!("Failed reading log content: {}", e))?;

        Ok(evaluate_session_log(
            &content,
            baseline,
            expected_mods,
            Utc::now(),
        ))
    }

    fn read_log_content_inner(&self) -> Result<String, String> {
        let path = self.log_path();
        if !path.exists() {
            return Ok(String::new());
        }
        std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read SMAPI log at '{}': {}", path.display(), e))
    }

    fn log_file_path_inner(&self) -> PathBuf {
        self.log_path()
    }
}

impl SessionLogPort for SmapiSessionLogReader {
    fn capture_baseline(&self) -> AppResult<SessionVerificationBaseline> {
        self.capture_baseline_inner()
            .map_err(|e| AppError::system("CAPTURE_BASELINE_FAILED", e))
    }

    fn verify_session(
        &self,
        baseline: &SessionVerificationBaseline,
        expected_mods: &[ExpectedMod],
    ) -> AppResult<SessionVerificationResult> {
        self.verify_session_inner(baseline, expected_mods)
            .map_err(|e| AppError::system("VERIFY_SESSION_FAILED", e))
    }

    fn read_log_content(&self) -> AppResult<String> {
        self.read_log_content_inner()
            .map_err(|e| AppError::system("READ_LOG_FAILED", e))
    }

    fn log_file_path(&self) -> PathBuf {
        self.log_file_path_inner()
    }

    fn log_is_available(&self) -> bool {
        self.log_path().is_file()
    }

    fn known_log_locations(&self) -> Vec<(String, String)> {
        crate::platform::host_semantics::known_smapi_log_locations()
    }

    fn other_logs(&self) -> Vec<manager_app::ports::logging::LogCandidate> {
        let current = self.log_path();
        let Some(dir) = current.parent() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found: Vec<_> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let meta = std::fs::symlink_metadata(&path).ok()?;
                let name = path.file_name()?.to_str()?.to_string();
                let is_log = meta.is_file()
                    && name.to_lowercase().starts_with("smapi")
                    && name.to_lowercase().ends_with(".txt");
                (is_log && path != current).then(|| manager_app::ports::logging::LogCandidate {
                    name,
                    modified_at: meta.modified().ok().map(chrono::DateTime::from),
                    size_bytes: meta.len(),
                })
            })
            .collect();
        found.sort_by_key(|a| std::cmp::Reverse(a.modified_at));
        found.truncate(20);
        found
    }

    fn read_other_log(&self, name: &str) -> AppResult<String> {
        let candidate = self
            .other_logs()
            .into_iter()
            .find(|c| c.name == name)
            .ok_or_else(|| {
                AppError::validation(
                    "LOG_NOT_FOUND",
                    format!("{name} is not one of the SMAPI logs found"),
                )
            })?;
        let dir = self
            .log_path()
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default();
        std::fs::read_to_string(dir.join(&candidate.name))
            .map_err(|e| AppError::system("READ_LOG_FAILED", format!("Could not read {name}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_core::ids::ModUniqueId;

    #[test]
    fn test_strip_smapi_prefix() {
        let line = "[12:34:56 INFO  SMAPI] Log started at 2026-09-13T12:34:56 UTC";
        let (msg, prefix) = strip_smapi_prefix(line);
        assert_eq!(msg, "Log started at 2026-09-13T12:34:56 UTC");
        let p = prefix.unwrap();
        assert_eq!(p.time_str, "12:34:56");
        assert_eq!(p.level, "INFO");
        assert_eq!(p.source, "SMAPI");

        let mod_line = "[12:34:57 TRACE Content Patcher] Loading patch...";
        let (m_msg, m_prefix) = strip_smapi_prefix(mod_line);
        assert_eq!(m_msg, "Loading patch...");
        let mp = m_prefix.unwrap();
        assert_eq!(mp.level, "TRACE");
        assert_eq!(mp.source, "Content Patcher");

        let no_prefix = "   Indented plain line";
        let (np_msg, np_prefix) = strip_smapi_prefix(no_prefix);
        assert_eq!(np_msg, no_prefix);
        assert!(np_prefix.is_none());
    }

    #[test]
    fn other_smapi_logs_are_offered_and_only_they_can_be_read() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("SMAPI-latest.txt"), "latest").unwrap();
        std::fs::write(tmp.path().join("SMAPI-crash.txt"), "crash").unwrap();
        std::fs::write(tmp.path().join("notes.txt"), "not a log").unwrap();
        std::fs::create_dir(tmp.path().join("SMAPI-dir.txt")).unwrap();
        let reader = SmapiSessionLogReader::new(Some(tmp.path().join("SMAPI-latest.txt")));
        let others = reader.other_logs();
        assert_eq!(
            others.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["SMAPI-crash.txt"]
        );
        assert_eq!(reader.read_other_log("SMAPI-crash.txt").unwrap(), "crash");
        assert!(reader.read_other_log("notes.txt").is_err());
        assert!(reader.read_other_log("../SMAPI-crash.txt").is_err());
        assert!(reader.read_other_log("SMAPI-latest.txt").is_err());
    }

    #[test]
    fn test_verify_session_with_skipped_mods() {
        let tmp = tempfile::tempdir().unwrap();
        let log_file = tmp.path().join("SMAPI-latest.txt");

        let now = Utc::now();
        let log_date_str = now.format("%Y-%m-%dT%H:%M:%S UTC").to_string();
        let log_content = format!(
            r#"[10:00:00 INFO  SMAPI] Log started at {}
[10:00:01 INFO  SMAPI] Loaded 1 mods:
[10:00:01 INFO  SMAPI]    GoodMod 1.0.0 by Author | Description
[10:00:02 ERROR SMAPI] Skipped mods:
[10:00:02 ERROR SMAPI]    BadMod 1.0 because it failed to load
"#,
            log_date_str
        );
        std::fs::write(&log_file, log_content).unwrap();

        let reader = SmapiSessionLogReader::new(Some(log_file.clone()));
        let baseline = SessionVerificationBaseline {
            log_path: log_file,
            initial_mtime: None,
            initial_size: 0,
            launch_time: now - chrono::Duration::seconds(5),
            initial_inode: None,
            expected_mods_path: None,
        };

        let res = SessionLogPort::verify_session(
            &reader,
            &baseline,
            &[ExpectedMod {
                unique_id: ModUniqueId::new("Author.GoodMod"),
                name: "GoodMod".to_string(),
                version: "1.0.0".to_string(),
            }],
        )
        .unwrap();
        assert!(res.session_matched);
        assert!(!res.all_mods_confirmed);
        assert!(res.error_details.unwrap().contains("BadMod"));
    }

    const MODS: &str = "/data/setups/abc/Mods";

    fn expected(id: &str, name: &str, version: &str) -> ExpectedMod {
        ExpectedMod {
            unique_id: ModUniqueId::new(id),
            name: name.to_string(),
            version: version.to_string(),
        }
    }

    fn profile_mods() -> Vec<ExpectedMod> {
        vec![
            expected("Pathoschild.ContentPatcher", "Content Patcher", "2.1.0"),
            expected("Weather.Plus", "Weather Plus", "3.1.0"),
            expected("Farm.Pack", "Fancy Farm", "1.0.0"),
        ]
    }

    fn baseline(launch: DateTime<Utc>) -> SessionVerificationBaseline {
        SessionVerificationBaseline {
            log_path: PathBuf::from("SMAPI-latest.txt"),
            initial_mtime: None,
            initial_size: 0,
            launch_time: launch,
            initial_inode: None,
            expected_mods_path: Some(PathBuf::from(MODS)),
        }
    }

    /// A log in SMAPI 4.1.10's shape, built from the format strings in its
    /// StardewModdingAPI.dll. `skip` moves a mod from the loaded lists into
    /// the skipped block; `mods_path` is what "Mods go here" says (`None`
    /// leaves the line out); `trace` controls the per-mod identity lines.
    fn smapi_log(
        start: DateTime<Utc>,
        mods_path: Option<&str>,
        skip: Option<&str>,
        trace: bool,
    ) -> String {
        let mut lines = vec![
            "[21:04:40 INFO  SMAPI] SMAPI 4.1.10 with Stardew Valley 1.6.15 build 24356 on Unix 6.12.113".to_string(),
        ];
        if let Some(path) = mods_path {
            lines.push(format!("[21:04:40 INFO  SMAPI] Mods go here: {path}"));
            lines.push("[21:04:40 TRACE SMAPI] (Using custom --mods-path argument.)".to_string());
        }
        lines.push(format!(
            "[21:04:40 TRACE SMAPI] Log started at {}",
            start.format("%Y-%m-%dT%H:%M:%S UTC")
        ));
        lines.push("[21:04:41 TRACE SMAPI] Loading mods...".to_string());
        if trace {
            lines.push("[21:04:41 TRACE SMAPI]    Content Patcher (from Mods/ContentPatcher/ContentPatcher.dll, ID: Pathoschild.ContentPatcher, assembly version: 2.1.0)...".to_string());
            lines.push("[21:04:41 TRACE SMAPI]    Weather Plus (from Mods/WeatherPlus/Plus.dll, ID: Weather.Plus, assembly version: 3.1.0)...".to_string());
            lines.push("[21:04:41 TRACE SMAPI]    Fancy Farm (from Mods/[CP] Fancy Farm, ID: Farm.Pack) [content pack]...".to_string());
        }
        let csharp: Vec<_> = [("Content Patcher", "2.1.0"), ("Weather Plus", "3.1.0")]
            .into_iter()
            .filter(|(name, _)| Some(*name) != skip)
            .collect();
        lines.push(format!(
            "[21:04:42 INFO  SMAPI] Loaded {} mods:",
            csharp.len()
        ));
        for (name, version) in &csharp {
            lines.push(format!(
                "[21:04:42 INFO  SMAPI]    {name} {version} by Someone | A mod"
            ));
        }
        lines.push("[21:04:42 INFO  SMAPI] Loaded 1 content packs:".to_string());
        lines.push(
            "[21:04:42 INFO  SMAPI]    Fancy Farm 1.0.0 by Someone | for Content Patcher | A farm"
                .to_string(),
        );
        if let Some(name) = skip {
            lines.push("[21:04:42 ERROR SMAPI]    Skipped mods".to_string());
            lines.push(format!("[21:04:42 ERROR SMAPI]    {}", "-".repeat(50)));
            lines.push(
                "[21:04:42 ERROR SMAPI]       These mods could not be added to your game."
                    .to_string(),
            );
            lines.push("[21:04:42 ERROR SMAPI] ".to_string());
            lines.push(format!(
                "[21:04:42 ERROR SMAPI]       - {name} 3.1.0 because it's no longer compatible. Please check for a new version at https://smapi.io/mods"
            ));
        }
        lines.push(
            "[21:04:43 INFO  SMAPI] Type 'help' for help, or 'help <cmd>' for a command's usage"
                .to_string(),
        );
        lines.join("\n")
    }

    fn evaluate(log: &str, launch: DateTime<Utc>) -> SessionVerificationResult {
        evaluate_session_log(
            log,
            &baseline(launch),
            &profile_mods(),
            launch + chrono::Duration::seconds(30),
        )
    }

    #[test]
    fn smapi_4_identity_lines_confirm_code_mods_and_content_packs() {
        let launch = Utc::now();
        let result = evaluate(&smapi_log(launch, Some(MODS), None, true), launch);
        assert_eq!(result.outcome, Some(LoadOutcome::Confirmed));
        assert!(result.all_mods_confirmed);
        assert!(result.verified_mods.iter().all(|m| m.found_in_log));
        assert!(result.custom_mods_path_detected);
    }

    #[test]
    fn identity_lines_in_both_shapes_are_parsed() {
        assert_eq!(
            parse_mod_identity("   Weather Plus (from Mods/W/Plus.dll, ID: Weather.Plus, assembly version: 3.1.0)..."),
            Some(("Weather Plus".to_string(), "Weather.Plus".to_string()))
        );
        assert_eq!(
            parse_mod_identity(
                "   Fancy Farm (from Mods/[CP] Fancy, Farm, ID: Farm.Pack) [content pack]..."
            ),
            Some(("Fancy Farm".to_string(), "Farm.Pack".to_string()))
        );
        assert_eq!(
            parse_mod_identity("Test Mod (ID: Author.TestMod)"),
            Some(("Test Mod".to_string(), "Author.TestMod".to_string()))
        );
        assert_eq!(parse_mod_identity("Loading mods..."), None);
    }

    #[test]
    fn a_skipped_mod_is_named_with_its_reason_and_the_rest_still_count() {
        let launch = Utc::now();
        let result = evaluate(
            &smapi_log(launch, Some(MODS), Some("Weather Plus"), true),
            launch,
        );
        assert_eq!(result.outcome, Some(LoadOutcome::Partial));
        assert_eq!(result.skipped_mods.len(), 1);
        assert_eq!(result.skipped_mods[0].name, "Weather Plus 3.1.0");
        assert!(result.skipped_mods[0]
            .reason
            .starts_with("it's no longer compatible"));
        assert!(result.unconfirmed_mods.is_empty());
        let confirmed: Vec<_> = result
            .verified_mods
            .iter()
            .filter(|m| m.found_in_log)
            .map(|m| m.unique_id.as_str())
            .collect();
        assert_eq!(confirmed, vec!["Pathoschild.ContentPatcher", "Farm.Pack"]);
        assert!(result.error_details.unwrap().contains("Weather Plus 3.1.0"));
    }

    #[test]
    fn an_anonymised_or_missing_mods_path_is_unavailable_not_a_failure() {
        let launch = Utc::now();
        for path in [Some("/home/****/.local/share/Mods"), None] {
            let result = evaluate(&smapi_log(launch, path, None, true), launch);
            assert_eq!(result.outcome, Some(LoadOutcome::Unavailable), "{path:?}");
            assert!(!result.all_mods_confirmed);
            // Named in a list that cannot be tied to the profile is not confirmed.
            assert!(result.verified_mods.iter().all(|m| !m.found_in_log));
            assert_eq!(result.unconfirmed_mods.len(), 3);
        }
    }

    #[test]
    fn a_log_without_mod_ids_is_unavailable() {
        let launch = Utc::now();
        let result = evaluate(&smapi_log(launch, Some(MODS), None, false), launch);
        assert_eq!(result.outcome, Some(LoadOutcome::Unavailable));
        assert_eq!(result.unconfirmed_mods.len(), 3);
    }

    #[test]
    fn a_mod_missing_from_the_loaded_list_is_partial_and_named() {
        let launch = Utc::now();
        let log = smapi_log(launch, Some(MODS), None, true).replace(
            "[21:04:42 INFO  SMAPI]    Weather Plus 3.1.0 by Someone | A mod\n",
            "",
        );
        let result = evaluate(&log, launch);
        assert_eq!(result.outcome, Some(LoadOutcome::Partial));
        assert_eq!(result.unconfirmed_mods, vec!["Weather Plus".to_string()]);
    }

    #[test]
    fn another_mods_folder_is_reported_as_such() {
        let launch = Utc::now();
        let result = evaluate(
            &smapi_log(launch, Some("/games/Stardew Valley/Mods"), None, true),
            launch,
        );
        assert_eq!(result.outcome, Some(LoadOutcome::WrongModsFolder));
        assert!(result
            .error_details
            .unwrap()
            .contains("/games/Stardew Valley/Mods"));
    }

    #[test]
    fn nothing_is_concluded_before_smapi_lists_its_mods() {
        let launch = Utc::now();
        let log = smapi_log(launch, Some(MODS), None, true);
        let loading = log.split("[21:04:42").next().unwrap();
        let result = evaluate(loading, launch);
        assert!(result.session_matched);
        assert_eq!(result.outcome, None);
    }

    #[test]
    fn an_earlier_sessions_log_does_not_match() {
        let launch = Utc::now();
        let log = smapi_log(
            launch - chrono::Duration::minutes(5),
            Some(MODS),
            None,
            true,
        );
        let result = evaluate(&log, launch);
        assert!(!result.session_matched);
        assert_eq!(result.outcome, None);
    }
}
