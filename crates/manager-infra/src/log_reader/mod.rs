use chrono::{DateTime, Utc};
use manager_app::error::{AppError, AppResult};
use manager_app::ports::logging::{ExpectedMod, SessionLogPort};
use manager_core::launch::{
    LoadOutcome, ModVerificationEvidence, SessionSkippedMod, SessionVerificationBaseline,
    SessionVerificationResult,
};
use manager_core::path_semantics::{host_path_semantics, PathSemantics};
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

/// Undoes SMAPI's `PathUtilities.AnonymizePathForDisplay`.
///
/// SMAPI writes a path inside the home folder (`HOME`, else `USERPROFILE`) as
/// `~` followed by the rest of the path, separator included:
/// `/home/luke/.local/share/x` is logged as `~/.local/share/x` and
/// `C:\Users\Luke\AppData\x` as `~\AppData\x`. Putting the same home folder
/// back in front is the exact reverse. `None` when the path is not written
/// that way, or the home folder is not known.
pub fn expand_smapi_home(observed: &str, home: Option<&Path>) -> Option<PathBuf> {
    let rest = observed.strip_prefix('~')?;
    if !(rest.starts_with('/') || rest.starts_with('\\')) {
        return None;
    }
    let home = home?.to_string_lossy();
    let home = home.trim_end_matches(['/', '\\']);
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(format!("{home}{rest}")))
}

/// Whether a path written in a log is absolute on the platform whose path
/// semantics apply. This is decided from the text rather than the host's
/// path parser, so a Windows log reads the same on every host:
/// `C:\...`, `C:/...` and `\\server\...` are absolute on Windows, `/...` on
/// POSIX.
fn is_absolute_for(semantics: &dyn PathSemantics, path: &str) -> bool {
    if semantics.id() == "windows" {
        let bytes = path.as_bytes();
        let drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        drive || path.starts_with(r"\\")
    } else {
        path.starts_with('/')
    }
}

fn compare_mods_folder(
    observed: Option<&str>,
    expected: Option<&Path>,
    home: Option<&Path>,
    semantics: &dyn PathSemantics,
) -> ModsFolderEvidence {
    let (Some(observed), Some(expected)) = (observed, expected) else {
        return ModsFolderEvidence::Unreadable;
    };
    // SMAPI itself abbreviates the home folder, and profile Mods folders are
    // always inside it.
    let expanded = expand_smapi_home(observed, home);
    let observed_path = expanded.as_deref().unwrap_or(Path::new(observed));
    // Log sharing tools replace the user name with asterisks, and some write
    // %USERPROFILE%. Such a path says nothing about which folder it was, and
    // neither does a `~` path whose home folder is not known.
    if observed.contains('*')
        || observed.contains('%')
        || !is_absolute_for(semantics, &observed_path.to_string_lossy())
    {
        return ModsFolderEvidence::Unreadable;
    }
    // SMAPI prints the path in its own separators and casing, so the
    // comparison follows the platform's path semantics rather than raw string
    // equality: C:\Users\Luke\... and c:/Users/Luke/... are the same directory.
    if semantics.paths_equivalent(observed_path, expected) {
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

/// Whether two versions are the same version as SMAPI sees them.
///
/// SMAPI prints a manifest's version normalised (`SemanticVersion.ToString`
/// always has major.minor.patch), so a manifest's `1.0` is logged as
/// `1.0.0` and `1.02` as `1.2.0`. Build metadata (`+...`) is ignored, as SMAPI
/// ignores it when comparing.
fn same_version(a: &str, b: &str) -> bool {
    let parse =
        |v: &str| manager_core::version::SmapiVersion::parse(v.split('+').next().unwrap_or(v)).ok();
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => a == b,
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

/// Whether one line of SMAPI's loaded list (`Name 1.0.0 by Author |
/// Description`, or for a content pack `... | for Framework | ...`) is
/// this mod at this version.
fn loaded_entry_matches(entry: &str, name: &str, version: &str) -> bool {
    let Some(rest) = entry.strip_prefix(name).and_then(|r| r.strip_prefix(' ')) else {
        return false;
    };
    let (printed, after) = rest.split_at(rest.find(' ').unwrap_or(rest.len()));
    (after.is_empty() || after.starts_with(" by ") || after.starts_with(" | "))
        && same_version(printed, version)
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
    home: Option<&Path>,
    now: DateTime<Utc>,
) -> SessionVerificationResult {
    evaluate_session_log_as(
        content,
        baseline,
        expected_mods,
        home,
        now,
        host_path_semantics(),
    )
}

/// `evaluate_session_log` with the path semantics of the platform the log
/// came from, so one platform's log can be read on another (and tested on
/// every host).
pub fn evaluate_session_log_as(
    content: &str,
    baseline: &SessionVerificationBaseline,
    expected_mods: &[ExpectedMod],
    home: Option<&Path>,
    now: DateTime<Utc>,
    semantics: &dyn PathSemantics,
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
        home,
        semantics,
    );

    let mut verified_mods = Vec::new();
    let mut evidence = Vec::new();
    for expected in expected_mods {
        let was_skipped = skipped.iter().any(|s| {
            s.name.eq_ignore_ascii_case(&expected.name)
                && s.version
                    .as_deref()
                    .is_none_or(|v| same_version(v, &expected.version))
        });
        let loaded_line = loaded_entries
            .iter()
            .find(|entry| loaded_entry_matches(entry, &expected.name, &expected.version));
        // Two expected mods with the same name and version cannot be told
        // apart by SMAPI's loaded list.
        let unambiguous = expected_mods
            .iter()
            .filter(|other| {
                other.name == expected.name && same_version(&other.version, &expected.version)
            })
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

        // Without the folder, nothing can be counted as loaded, so a skipped
        // mod is reported (it is in `skipped_mods`) without calling the
        // session partial.
        let outcome = if !folder_known {
            LoadOutcome::Unavailable
        } else if !skipped_mods.is_empty() || !not_seen.is_empty() {
            LoadOutcome::Partial
        } else if !unconfirmable.is_empty() {
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
    /// The home folder SMAPI writes as `~` in its log.
    home: Option<PathBuf>,
}

impl SmapiSessionLogReader {
    /// A reader that resolves the log through the platform locator.
    pub fn new(custom_log_path: Option<PathBuf>) -> Self {
        Self {
            locator: Box::new(HostSmapiLogLocator),
            custom_log_path,
            home: crate::platform::smapi_home_folder(),
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
            home: crate::platform::smapi_home_folder(),
        }
    }

    /// The home folder to put back where SMAPI's log says `~`, for tests that
    /// model another user's machine.
    pub fn with_home(mut self, home: Option<PathBuf>) -> Self {
        self.home = home;
        self
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
            self.home.as_deref(),
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

    /// One platform's SMAPI 4.1.10 session log (see the fixtures' tests in
    /// manager-core's log_summary) and the facts needed to read it. SMAPI
    /// writes the Mods path relative to the home folder: `~/...` under HOME
    /// on Linux and macOS, `~\...` under USERPROFILE on Windows, where it also
    /// writes CRLF line endings. Every host reads both logs with their own
    /// platform's path semantics, so each platform keeps real coverage
    /// wherever the tests run.
    struct Platform {
        name: &'static str,
        log: &'static str,
        nl: &'static str,
        semantics: &'static dyn PathSemantics,
        home: &'static str,
        other_home: &'static str,
        mods: &'static str,
        mods_as_logged: &'static str,
        anonymised: &'static str,
        game_mods_as_logged: &'static str,
    }

    static POSIX: Platform = Platform {
        name: "posix",
        log: include_str!("../../../manager-core/tests/fixtures/SMAPI-4.1.10-session.txt"),
        nl: "\n",
        semantics: &manager_core::path_semantics::PosixPathSemantics,
        home: "/home/player",
        other_home: "/home/someone-else",
        mods: "/home/player/.local/share/stardew-mod-manager/setups/0b6f5c1e-2d3a-4f5b-9c8d-7e6f5a4b3c2d/Mods",
        mods_as_logged:
            "~/.local/share/stardew-mod-manager/setups/0b6f5c1e-2d3a-4f5b-9c8d-7e6f5a4b3c2d/Mods",
        anonymised: "/home/****/.local/share/stardew-mod-manager/setups/0b6f5c1e-2d3a-4f5b-9c8d-7e6f5a4b3c2d/Mods",
        game_mods_as_logged: "~/.local/share/Steam/steamapps/common/Stardew Valley/Mods",
    };

    static WINDOWS: Platform = Platform {
        name: "windows",
        log: include_str!("../../../manager-core/tests/fixtures/SMAPI-4.1.10-session-windows.txt"),
        nl: "\r\n",
        semantics: &manager_core::path_semantics::WindowsPathSemantics,
        home: r"C:\Users\Player",
        other_home: r"C:\Users\SomeoneElse",
        mods: r"C:\Users\Player\AppData\Roaming\stardew-mod-manager\data\setups\0b6f5c1e-2d3a-4f5b-9c8d-7e6f5a4b3c2d\Mods",
        mods_as_logged: r"~\AppData\Roaming\stardew-mod-manager\data\setups\0b6f5c1e-2d3a-4f5b-9c8d-7e6f5a4b3c2d\Mods",
        anonymised: r"C:\Users\****\AppData\Roaming\stardew-mod-manager\data\setups\0b6f5c1e-2d3a-4f5b-9c8d-7e6f5a4b3c2d\Mods",
        game_mods_as_logged: r"C:\Program Files (x86)\Steam\steamapps\common\Stardew Valley\Mods",
    };

    const PLATFORMS: [&Platform; 2] = [&POSIX, &WINDOWS];

    /// The platform this build runs on, for the test that goes through the
    /// real reader and the host's own path handling.
    fn host() -> &'static Platform {
        if cfg!(windows) {
            &WINDOWS
        } else {
            &POSIX
        }
    }

    const WEATHER_LOADED: &str =
        "[10:00:07 INFO  SMAPI]    Weather Plus 3.1.0 by Someone | Weather.";

    impl Platform {
        /// One log line with this platform's line ending.
        fn line(&self, text: &str) -> String {
            format!("{text}{}", self.nl)
        }

        fn baseline(&self) -> SessionVerificationBaseline {
            SessionVerificationBaseline {
                log_path: PathBuf::from("SMAPI-latest.txt"),
                initial_mtime: None,
                initial_size: 0,
                launch_time: launch(),
                initial_inode: None,
                expected_mods_path: Some(PathBuf::from(self.mods)),
            }
        }

        fn evaluate_with(
            &self,
            log: &str,
            expected: &[ExpectedMod],
            home: Option<&str>,
        ) -> SessionVerificationResult {
            evaluate_session_log_as(
                log,
                &self.baseline(),
                expected,
                home.map(Path::new),
                launch() + chrono::Duration::minutes(5),
                self.semantics,
            )
        }

        fn evaluate(&self, log: &str) -> SessionVerificationResult {
            self.evaluate_with(log, &profile_mods(), Some(self.home))
        }

        /// The fixture without its skipped mod: Weather Plus loads like the rest.
        fn all_loaded(&self) -> String {
            let skipped_block = [
                "[10:00:07 ERROR SMAPI]    Skipped mods",
                "[10:00:07 ERROR SMAPI]    --------------------------------------------------",
                "[10:00:07 ERROR SMAPI]       These mods could not be added to your game.",
                "",
                "[10:00:07 ERROR SMAPI]       - Weather Plus 3.1.0 because its DLL couldn't be loaded.",
                "[10:00:07 TRACE SMAPI]         (Error: System.BadImageFormatException: Bad IL format. The format of the file 'Plus.dll' is invalid.)",
                "",
            ]
            .map(|text| self.line(text))
            .concat();
            let tiny = "[10:00:07 INFO  SMAPI]    Tiny Tweaks 1.0.0 by Someone | Small changes.";
            self.log
                .replace(
                    &self.line("[10:00:07 INFO  SMAPI] Loaded 2 mods:"),
                    &self.line("[10:00:07 INFO  SMAPI] Loaded 3 mods:"),
                )
                .replace(
                    &self.line(tiny),
                    &(self.line(tiny) + &self.line(WEATHER_LOADED)),
                )
                .replace(&skipped_block, "")
        }
    }

    fn expected(id: &str, name: &str, version: &str) -> ExpectedMod {
        ExpectedMod {
            unique_id: ModUniqueId::new(id),
            name: name.to_string(),
            version: version.to_string(),
        }
    }

    /// The profile behind the fixture. Tiny Tweaks' manifest says "1.0",
    /// which SMAPI prints as 1.0.0.
    fn profile_mods() -> Vec<ExpectedMod> {
        vec![
            expected("Pathoschild.ContentPatcher", "Content Patcher", "2.1.0"),
            expected("Tiny.Tweaks", "Tiny Tweaks", "1.0"),
            expected("Farm.Pack", "Fancy Farm", "1.0.0"),
            expected("Weather.Plus", "Weather Plus", "3.1.0"),
        ]
    }

    /// The fixtures' session started at 10:00:05 UTC on 2026-11-03.
    fn launch() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-03T10:00:04Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn confirmed(result: &SessionVerificationResult) -> Vec<&str> {
        result
            .verified_mods
            .iter()
            .filter(|m| m.found_in_log)
            .map(|m| m.unique_id.as_str())
            .collect()
    }

    #[test]
    fn each_fixture_has_its_platforms_line_endings_and_paths() {
        // .gitattributes keeps the fixtures byte-exact on every checkout.
        assert!(!POSIX.log.contains('\r'));
        assert_eq!(
            WINDOWS.log.matches("\r\n").count(),
            WINDOWS.log.matches('\n').count()
        );
        for p in PLATFORMS {
            assert!(
                p.log.contains(&p.line(&format!(
                    "[10:00:05 INFO  SMAPI] Mods go here: {}",
                    p.mods_as_logged
                ))),
                "{}",
                p.name
            );
        }
    }

    #[test]
    fn the_fixture_is_the_shape_this_test_edits() {
        for p in PLATFORMS {
            let log = p.all_loaded();
            assert_ne!(log, p.log, "{}", p.name);
            assert!(!log.contains("Skipped mods"), "{}", p.name);
            assert!(log.contains(&p.line(WEATHER_LOADED)), "{}", p.name);
        }
    }

    #[test]
    fn the_reader_verifies_the_session_file_with_its_home_folder() {
        // Through the real reader, file and host path handling, so this one
        // reads the host platform's log.
        let p = host();
        let tmp = tempfile::tempdir().unwrap();
        let log_file = tmp.path().join("SMAPI-latest.txt");
        let now = Utc::now();
        let content = p.log.replace(
            "Log started at 2026-11-03T10:00:05 UTC",
            &format!("Log started at {}", now.format("%Y-%m-%dT%H:%M:%S UTC")),
        );
        std::fs::write(&log_file, content).unwrap();

        let reader = SmapiSessionLogReader::new(Some(log_file.clone()))
            .with_home(Some(PathBuf::from(p.home)));
        let baseline = SessionVerificationBaseline {
            log_path: log_file,
            launch_time: now - chrono::Duration::seconds(5),
            ..p.baseline()
        };
        let res = SessionLogPort::verify_session(&reader, &baseline, &profile_mods()).unwrap();
        assert!(res.session_matched);
        assert_eq!(res.outcome, Some(LoadOutcome::Partial));
        assert_eq!(res.skipped_mods.len(), 1);
        assert!(res.error_details.unwrap().contains("Weather Plus 3.1.0"));
    }

    #[test]
    fn a_real_smapi_session_with_a_home_relative_mods_path_is_confirmed() {
        for p in PLATFORMS {
            let result = p.evaluate(&p.all_loaded());
            assert_eq!(
                result.outcome,
                Some(LoadOutcome::Confirmed),
                "{}: {:?}",
                p.name,
                result.error_details
            );
            assert!(result.all_mods_confirmed);
            assert_eq!(confirmed(&result).len(), 4, "{}", p.name);
            assert!(result.custom_mods_path_detected);
        }
    }

    #[test]
    fn a_windows_path_compares_without_case_or_separator_differences() {
        // SMAPI normalises separators; Windows paths are case-insensitive.
        let log = WINDOWS.all_loaded().replace(
            WINDOWS.mods_as_logged,
            r"~/appdata/roaming/Stardew-Mod-Manager/data/setups/0b6f5c1e-2d3a-4f5b-9c8d-7e6f5a4b3c2d/mods",
        );
        let result = WINDOWS.evaluate(&log);
        assert_eq!(result.outcome, Some(LoadOutcome::Confirmed));
        // A POSIX filesystem is case-sensitive, so there it is another folder.
        let log = POSIX.all_loaded().replace("/Mods", "/mods");
        assert_eq!(
            POSIX.evaluate(&log).outcome,
            Some(LoadOutcome::WrongModsFolder)
        );
    }

    #[test]
    fn one_skipped_mod_is_one_skipped_mod_and_the_rest_are_confirmed() {
        for p in PLATFORMS {
            let result = p.evaluate(p.log);
            assert_eq!(result.outcome, Some(LoadOutcome::Partial), "{}", p.name);
            assert_eq!(
                result.skipped_mods,
                vec![SessionSkippedMod {
                    name: "Weather Plus 3.1.0".to_string(),
                    reason: "its DLL couldn't be loaded.".to_string(),
                }]
            );
            assert_eq!(
                confirmed(&result),
                vec!["Pathoschild.ContentPatcher", "Tiny.Tweaks", "Farm.Pack"]
            );
            assert!(result.unconfirmed_mods.is_empty());
            let details = result.error_details.unwrap();
            assert!(details.contains("Weather Plus 3.1.0"), "{details}");
            assert!(!details.contains("Patched game code"), "{details}");
        }
    }

    #[test]
    fn a_two_part_manifest_version_matches_smapis_three_part_one() {
        assert!(same_version("1.0", "1.0.0"));
        assert!(same_version("1.02", "1.2.0"));
        assert!(same_version("2.1.0-beta.2", "2.1-beta.2"));
        assert!(!same_version("2.1.0", "2.1.1"));
        assert!(!same_version("2.1.0-beta.2", "2.1.0"));
        for p in PLATFORMS {
            let result = p.evaluate(&p.all_loaded());
            assert!(confirmed(&result).contains(&"Tiny.Tweaks"), "{}", p.name);
        }
    }

    #[test]
    fn the_home_folder_is_put_back_exactly_as_smapi_removed_it() {
        assert_eq!(
            expand_smapi_home("~/.local/share/x/Mods", Some(Path::new("/home/player"))),
            Some(PathBuf::from("/home/player/.local/share/x/Mods"))
        );
        assert_eq!(
            expand_smapi_home("~/x", Some(Path::new("/home/player/"))),
            Some(PathBuf::from("/home/player/x"))
        );
        // Windows: SMAPI uses USERPROFILE and backslashes.
        assert_eq!(
            expand_smapi_home(
                r"~\AppData\Roaming\stardew-mod-manager\setups\a\Mods",
                Some(Path::new(r"C:\Users\Player"))
            ),
            Some(PathBuf::from(
                r"C:\Users\Player\AppData\Roaming\stardew-mod-manager\setups\a\Mods"
            ))
        );
        // Only "~" followed by a separator is SMAPI's abbreviation.
        assert_eq!(
            expand_smapi_home("~player/x", Some(Path::new("/home"))),
            None
        );
        assert_eq!(expand_smapi_home("/abs/x", Some(Path::new("/home"))), None);
        assert_eq!(expand_smapi_home("~/x", None), None);
    }

    #[test]
    fn absolute_paths_are_read_by_the_logs_platform() {
        let posix = &manager_core::path_semantics::PosixPathSemantics;
        let windows = &manager_core::path_semantics::WindowsPathSemantics;
        assert!(is_absolute_for(posix, "/home/player/Mods"));
        assert!(!is_absolute_for(posix, r"C:\Users\Player\Mods"));
        assert!(!is_absolute_for(posix, "~/Mods"));
        assert!(is_absolute_for(windows, r"C:\Users\Player\Mods"));
        assert!(is_absolute_for(windows, "c:/Users/Player/Mods"));
        assert!(is_absolute_for(windows, r"\\server\share\Mods"));
        assert!(!is_absolute_for(windows, "/home/player/Mods"));
        assert!(!is_absolute_for(windows, r"~\Mods"));
        assert!(!is_absolute_for(windows, r"C:Mods"));
    }

    #[test]
    fn a_home_relative_path_under_another_home_is_not_confirmed() {
        // The same ~ path read against another user's home is another folder.
        for p in PLATFORMS {
            let result = p.evaluate_with(&p.all_loaded(), &profile_mods(), Some(p.other_home));
            assert_eq!(
                result.outcome,
                Some(LoadOutcome::WrongModsFolder),
                "{}",
                p.name
            );
            assert!(confirmed(&result).is_empty());
        }
    }

    #[test]
    fn a_home_relative_path_without_a_known_home_is_unavailable() {
        for p in PLATFORMS {
            let result = p.evaluate_with(&p.all_loaded(), &profile_mods(), None);
            assert_eq!(result.outcome, Some(LoadOutcome::Unavailable), "{}", p.name);
            assert!(confirmed(&result).is_empty());
        }
    }

    #[test]
    fn identity_lines_in_both_shapes_are_parsed() {
        assert_eq!(
            parse_mod_identity("   Weather Plus (from Mods/W/Plus.dll, ID: Weather.Plus, assembly version: 3.1.0)..."),
            Some(("Weather Plus".to_string(), "Weather.Plus".to_string()))
        );
        assert_eq!(
            parse_mod_identity(
                r"   Weather Plus (from Mods\W\Plus.dll, ID: Weather.Plus, assembly version: 3.1.0)..."
            ),
            Some(("Weather Plus".to_string(), "Weather.Plus".to_string()))
        );
        assert_eq!(
            parse_mod_identity(
                "   Fancy Farm (from Mods/[CP] Fancy, Farm, ID: Farm.Pack) [content pack]..."
            ),
            Some(("Fancy Farm".to_string(), "Farm.Pack".to_string()))
        );
        assert_eq!(
            parse_mod_identity("   No Dll (from Mods/NoDll, ID: No.Dll)..."),
            Some(("No Dll".to_string(), "No.Dll".to_string()))
        );
        assert_eq!(
            parse_mod_identity("Test Mod (ID: Author.TestMod)"),
            Some(("Test Mod".to_string(), "Author.TestMod".to_string()))
        );
        assert_eq!(parse_mod_identity("Loading mods..."), None);
    }

    #[test]
    fn an_unidentifiable_mods_folder_is_unavailable_even_with_a_skipped_mod() {
        // Review S4: "SMAPI confirmed 0 of 4" is not evidence that nothing
        // loaded. The skipped mod is still reported.
        for p in PLATFORMS {
            for path in [p.anonymised, "%USERPROFILE%/Mods"] {
                let log = p.log.replace(p.mods_as_logged, path);
                assert_ne!(log, p.log);
                let result = p.evaluate(&log);
                assert_eq!(
                    result.outcome,
                    Some(LoadOutcome::Unavailable),
                    "{}: {path}",
                    p.name
                );
                assert!(confirmed(&result).is_empty());
                assert_eq!(result.skipped_mods.len(), 1);
                assert_eq!(result.unconfirmed_mods.len(), 3);
            }
            let without = p.log.replace(
                &p.line(&format!(
                    "[10:00:05 INFO  SMAPI] Mods go here: {}",
                    p.mods_as_logged
                )),
                "",
            );
            assert_ne!(without, p.log);
            assert_eq!(p.evaluate(&without).outcome, Some(LoadOutcome::Unavailable));
        }
    }

    #[test]
    fn a_log_without_mod_ids_is_unavailable() {
        for p in PLATFORMS {
            let log: String = p
                .all_loaded()
                .lines()
                .filter(|line| !line.contains(", ID: "))
                .map(|line| p.line(line))
                .collect();
            let result = p.evaluate(&log);
            assert_eq!(result.outcome, Some(LoadOutcome::Unavailable), "{}", p.name);
            assert_eq!(result.unconfirmed_mods.len(), 4, "{}", p.name);
        }
    }

    #[test]
    fn a_mod_missing_from_the_loaded_list_is_partial_and_named() {
        for p in PLATFORMS {
            let log = p.all_loaded().replace(&p.line(WEATHER_LOADED), "");
            let result = p.evaluate(&log);
            assert_eq!(result.outcome, Some(LoadOutcome::Partial), "{}", p.name);
            assert_eq!(result.unconfirmed_mods, vec!["Weather Plus".to_string()]);
        }
    }

    #[test]
    fn another_mods_folder_is_reported_as_such() {
        for p in PLATFORMS {
            let log = p.log.replace(p.mods_as_logged, p.game_mods_as_logged);
            let result = p.evaluate(&log);
            assert_eq!(
                result.outcome,
                Some(LoadOutcome::WrongModsFolder),
                "{}",
                p.name
            );
            assert!(result
                .error_details
                .unwrap()
                .contains(p.game_mods_as_logged));
        }
    }

    #[test]
    fn nothing_is_concluded_before_smapi_lists_its_mods() {
        for p in PLATFORMS {
            let loading = p.log.split("[10:00:07").next().unwrap();
            let result = p.evaluate(loading);
            assert!(result.session_matched);
            assert_eq!(result.outcome, None, "{}", p.name);
        }
    }

    #[test]
    fn an_earlier_sessions_log_does_not_match() {
        for p in PLATFORMS {
            let result = evaluate_session_log_as(
                p.log,
                &SessionVerificationBaseline {
                    launch_time: launch() + chrono::Duration::minutes(10),
                    ..p.baseline()
                },
                &profile_mods(),
                Some(Path::new(p.home)),
                launch() + chrono::Duration::minutes(15),
                p.semantics,
            );
            assert!(!result.session_matched);
            assert_eq!(result.outcome, None);
        }
    }
}
