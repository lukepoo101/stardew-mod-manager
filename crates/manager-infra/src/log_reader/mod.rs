use chrono::{DateTime, Utc};
use manager_app::error::{AppError, AppResult};
use manager_app::ports::logging::{ExpectedMod, SessionLogPort};
use manager_core::launch::{
    ModVerificationEvidence, SessionVerificationBaseline, SessionVerificationResult,
};
use manager_core::path_semantics::host_path_semantics;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

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
                session_matched: false,
                session_start_time: None,
                custom_mods_path_detected: false,
                all_mods_confirmed: false,
                verified_mods: Vec::new(),
                error_details: Some("Log file does not exist yet".to_string()),
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
                session_matched: false,
                session_start_time: None,
                custom_mods_path_detected: false,
                all_mods_confirmed: false,
                verified_mods: Vec::new(),
                error_details: Some(
                    "Log file has not been modified since game launch (stale log)".to_string(),
                ),
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

        let mut observed_mods_path: Option<String> = None;
        let mut session_matched = false;
        let mut session_start_time = None;
        let mut custom_mods_path_detected = false;
        let mut skipped_mods_detected = false;
        let mut skipped_mod_names = Vec::new();
        let mut loaded_mod_lines = Vec::new();
        let mut in_skipped_section = false;
        let mut in_loaded_section = false;

        for line in content.lines() {
            let (msg, prefix) = strip_smapi_prefix(line);

            if prefix.as_ref().is_none_or(|p| p.source != "SMAPI") {
                continue;
            }
            if let Some(path) = msg.strip_prefix("Mods go here: ") {
                observed_mods_path = Some(path.trim().to_string());
            }
            if msg.contains("Log started at") {
                if let Some(pos) = msg.find("Log started at ") {
                    let date_str = &msg[pos + 15..];
                    let parsed_opt = parse_smapi_date(date_str);
                    let is_match = if let Some(parsed) = parsed_opt {
                        // Allow 2 seconds skew for whole second precision
                        parsed.timestamp() >= baseline.launch_time.timestamp()
                            && parsed <= Utc::now()
                    } else {
                        false
                    };

                    if is_match {
                        session_matched = true;
                        session_start_time = parsed_opt;
                        // Reset session tracking for new session instance
                        custom_mods_path_detected = false;
                        skipped_mods_detected = false;
                        skipped_mod_names.clear();
                        loaded_mod_lines.clear();
                        in_skipped_section = false;
                        in_loaded_section = false;
                    }
                }
            }

            if !session_matched {
                continue;
            }

            if msg.contains("--mods-path") || msg.contains("Using custom --mods-path argument") {
                custom_mods_path_detected = true;
            }

            if msg.starts_with("Skipped mods:") {
                skipped_mods_detected = true;
                in_skipped_section = true;
                in_loaded_section = false;
                continue;
            }

            if msg.starts_with("Loaded ")
                && (msg.contains("mods:") || msg.contains("content packs:"))
            {
                in_loaded_section = true;
                in_skipped_section = false;
                continue;
            }

            let is_indented = line.starts_with("   ")
                || msg.starts_with("   ")
                || line.starts_with('\t')
                || msg.starts_with('\t');

            if in_skipped_section {
                if is_indented {
                    skipped_mod_names.push(msg.trim().to_string());
                } else if !msg.is_empty() && prefix.is_some() {
                    in_skipped_section = false;
                }
            }

            if in_loaded_section {
                if is_indented {
                    loaded_mod_lines.push(line.to_string());
                } else if !msg.is_empty() && prefix.is_some() {
                    in_loaded_section = false;
                }
            }
        }

        if !session_matched {
            return Ok(SessionVerificationResult {
                session_matched: false,
                session_start_time: None,
                custom_mods_path_detected,
                all_mods_confirmed: false,
                verified_mods: Vec::new(),
                error_details: Some(
                    "Session start timestamp could not be verified in log".to_string(),
                ),
            });
        }

        if skipped_mods_detected {
            let details = if !skipped_mod_names.is_empty() {
                format!("SMAPI skipped mod(s): {}", skipped_mod_names.join("; "))
            } else {
                "SMAPI reported skipped mods during launch".to_string()
            };
            return Ok(SessionVerificationResult {
                session_matched: true,
                session_start_time,
                custom_mods_path_detected,
                all_mods_confirmed: false,
                verified_mods: Vec::new(),
                error_details: Some(details),
            });
        }

        let mut verified_mods = Vec::new();
        let mut all_confirmed = baseline
            .expected_mods_path
            .as_ref()
            .is_some_and(|expected| {
                observed_mods_path.as_ref().is_some_and(|observed| {
                    // SMAPI prints the path in its own separators and casing, so
                    // the comparison follows host path semantics rather than
                    // raw string equality: C:\Users\Luke\... and
                    // c:/Users/Luke/... are the same directory.
                    host_path_semantics().paths_equivalent(std::path::Path::new(observed), expected)
                })
            });

        for expected in expected_mods {
            let mod_id = expected.unique_id.as_str();
            let mod_name = expected.name.as_str();
            let mod_ver = expected.version.as_str();

            let mut found = false;
            let mut matching_line = None;

            let unambiguous = expected_mods
                .iter()
                .filter(|other| other.name == mod_name && other.version == mod_ver)
                .count()
                == 1;
            let has_identity = content.lines().any(|line| {
                let (msg, prefix) = strip_smapi_prefix(line);
                prefix.is_some_and(|p| p.source == "SMAPI" && p.level == "TRACE")
                    && msg.contains(&format!("ID: {mod_id})"))
            });
            for line in &loaded_mod_lines {
                let (msg, prefix) = strip_smapi_prefix(line);
                let expected = format!("{mod_name} {mod_ver}");
                let entry = msg.trim_start();
                let exact = entry.strip_prefix(&expected).is_some_and(|rest| {
                    rest.is_empty() || rest.starts_with(" by ") || rest.starts_with(" | ")
                });
                if exact
                    && unambiguous
                    && has_identity
                    && prefix.is_some_and(|p| p.source == "SMAPI" && p.level == "INFO")
                {
                    found = true;
                    matching_line = Some(line.clone());
                    break;
                }
            }

            if !found {
                all_confirmed = false;
            }

            verified_mods.push(ModVerificationEvidence {
                unique_id: mod_id.to_string(),
                expected_version: mod_ver.to_string(),
                found_in_log: found,
                log_entry: matching_line,
            });
        }

        let error_details = if !all_confirmed {
            Some("One or more expected mods were not loaded by SMAPI".to_string())
        } else {
            None
        };

        Ok(SessionVerificationResult {
            session_matched: true,
            session_start_time,
            custom_mods_path_detected,
            all_mods_confirmed: all_confirmed,
            verified_mods,
            error_details,
        })
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
}
