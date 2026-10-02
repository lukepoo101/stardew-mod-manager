use crate::api::dto::{sort_findings, DiagnosticsDto, FindingDto, PlatformPathDto};
use crate::error::AppResult;
use crate::ports::logging::SessionLogPort;
use crate::ports::repositories::LaunchSessionRepository;
use manager_core::game::OperatingSystem;
use manager_core::ids::LaunchSessionId;
use manager_core::smapi::summarize_log;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Static facts about the running installation, gathered by the composition root.
///
/// Diagnostics reports what the binary actually is rather than letting the UI
/// guess from the operating system it happens to be displayed on.
#[derive(Debug, Clone)]
pub struct HostEnvironment {
    pub operating_system: OperatingSystem,
    pub app_data_dir: PathBuf,
    pub cache_dir: PathBuf,
    /// The Steam roots the discovery for this host would inspect.
    pub steam_roots: Vec<PathBuf>,
}

pub struct DiagnosticsService {
    session_repo: Arc<dyn LaunchSessionRepository>,
    log_reader: Arc<dyn SessionLogPort>,
    environment: HostEnvironment,
}

impl DiagnosticsService {
    pub fn new(
        session_repo: Arc<dyn LaunchSessionRepository>,
        log_reader: Arc<dyn SessionLogPort>,
        environment: HostEnvironment,
    ) -> Self {
        Self {
            session_repo,
            log_reader,
            environment,
        }
    }

    pub fn get_diagnostics(
        &self,
        session_id: Option<&LaunchSessionId>,
    ) -> AppResult<DiagnosticsDto> {
        let raw_log = self.log_reader.read_log_content().unwrap_or_default();
        let log_path = self
            .log_reader
            .log_file_path()
            .to_string_lossy()
            .to_string();

        let session = if let Some(sid) = session_id {
            self.session_repo.get_launch_session(sid)?
        } else {
            self.session_repo.get_latest_launch_session(None)?
        };

        let session_state_str = session
            .as_ref()
            .map(|s| format!("{:?}", s.state).to_lowercase());
        let session_id_str = session.as_ref().map(|s| s.id.to_string());

        let summary = summarize_log(&raw_log);
        let now = chrono::Utc::now().to_rfc3339();
        let mut findings = Vec::new();

        for skipped in &summary.skipped_mods {
            let version = skipped
                .version
                .as_deref()
                .map(|v| format!(" {v}"))
                .unwrap_or_default();
            let mut evidence = vec![format!(
                "SMAPI log line {}: {}",
                skipped.line, skipped.reason
            )];
            for dependency in &skipped.missing_dependencies {
                evidence.push(format!("Needs {dependency}, which SMAPI could not find"));
            }
            findings.push(FindingDto {
                id: uuid::Uuid::new_v4().to_string(),
                fingerprint: format!("log_mod_skipped_{}{}", skipped.name, version),
                code: "LOG_MOD_SKIPPED".to_string(),
                severity: "error".to_string(),
                category: "runtime".to_string(),
                title: format!("{}{} did not load", skipped.name, version),
                summary: if skipped.reason.is_empty() {
                    format!("SMAPI skipped {}{}.", skipped.name, version)
                } else {
                    format!(
                        "SMAPI skipped {}{}: {}",
                        skipped.name, version, skipped.reason
                    )
                },
                affected_entities: vec![skipped.name.clone()],
                evidence,
                observed_at: now.clone(),
            });
        }

        for source in summary.sources.iter().filter(|s| s.errors > 0) {
            findings.push(FindingDto {
                id: uuid::Uuid::new_v4().to_string(),
                fingerprint: format!("log_source_errors_{}", source.source),
                code: "LOG_MOD_ERRORS".to_string(),
                severity: "warning".to_string(),
                category: "runtime".to_string(),
                title: format!("{} reported errors", source.source),
                summary: format!(
                    "{} logged {} error(s) and {} warning(s) in the latest session.",
                    source.source, source.errors, source.warnings
                ),
                affected_entities: vec![source.source.clone()],
                evidence: source
                    .first_error_line
                    .map(|line| vec![format!("First error at SMAPI log line {line}")])
                    .unwrap_or_default(),
                observed_at: now.clone(),
            });
        }

        if !summary.update_notices.is_empty() {
            findings.push(FindingDto {
                id: uuid::Uuid::new_v4().to_string(),
                fingerprint: "log_update_notices".to_string(),
                code: "LOG_UPDATES_REPORTED".to_string(),
                severity: "info".to_string(),
                category: "updates".to_string(),
                title: "SMAPI reports newer versions".to_string(),
                summary: format!(
                    "SMAPI says {} installed mod(s) have newer versions. The manager did not check this itself.",
                    summary.update_notices.len()
                ),
                affected_entities: summary
                    .update_notices
                    .iter()
                    .map(|n| n.name.clone())
                    .collect(),
                evidence: summary
                    .update_notices
                    .iter()
                    .map(|n| {
                        format!(
                            "{} {} to {} (log line {})",
                            n.name, n.current_version, n.available_version, n.line
                        )
                    })
                    .collect(),
                observed_at: now.clone(),
            });
        }

        // Older or unusual logs may signal a failure in ways the structured
        // reading does not recognise; say so rather than reporting nothing.
        if findings
            .iter()
            .all(|f| f.code != "LOG_MOD_SKIPPED" && f.code != "LOG_MOD_ERRORS")
            && (raw_log.contains("[SMAPI]    Failed:") || raw_log.contains("An error occurred"))
        {
            findings.push(FindingDto {
                id: uuid::Uuid::new_v4().to_string(),
                fingerprint: "smapi_log_error".to_string(),
                code: "LOG_ERROR_DETECTED".to_string(),
                severity: "warning".to_string(),
                category: "runtime".to_string(),
                title: "Errors Detected in SMAPI Log".to_string(),
                summary: "One or more error lines were detected in the latest SMAPI log output."
                    .to_string(),
                affected_entities: Vec::new(),
                evidence: vec!["Errors found in SMAPI-latest.txt".to_string()],
                observed_at: now.clone(),
            });
        }
        if !self.log_reader.log_is_available() {
            findings.push(FindingDto {
                id: uuid::Uuid::new_v4().to_string(),
                fingerprint: "smapi_log_missing".to_string(),
                code: "SMAPI_LOG_MISSING".to_string(),
                severity: "info".to_string(),
                category: "runtime".to_string(),
                title: "No SMAPI log found".to_string(),
                summary: format!(
                    "Nothing is readable at the {} SMAPI log location yet. It is created the first time SMAPI starts.",
                    self.environment.operating_system.as_key()
                ),
                affected_entities: Vec::new(),
                evidence: vec![log_path.clone()],
                observed_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        sort_findings(&mut findings);

        let log_started_at = log_start_time(&raw_log);
        let log_match = match (raw_log.trim().is_empty(), &session, log_started_at) {
            (true, _, _) => "none",
            (false, None, _) => "unmatched",
            (false, Some(_), None) => "unknown",
            // A little slack: SMAPI writes the time in whole seconds.
            (false, Some(s), Some(started))
                if started.timestamp() + 2 >= s.launched_at.timestamp() =>
            {
                "current_session"
            }
            (false, Some(_), Some(_)) => "stale",
        };

        Ok(DiagnosticsDto {
            log_match: log_match.to_string(),
            log_started_at: log_started_at.map(|at| at.to_rfc3339()),
            log_summary: summary.into(),
            session_id: session_id_str,
            session_state: session_state_str,
            findings,
            raw_log,
            log_file_path: log_path,
            host_operating_system: self.environment.operating_system.as_key().to_string(),
            app_data_dir: render(&self.environment.app_data_dir),
            cache_dir: render(&self.environment.cache_dir),
            steam_installations_checked: self
                .environment
                .steam_roots
                .iter()
                .map(|root| render(root))
                .collect(),
            smapi_log_locations: self.smapi_log_locations(),
        })
    }

    /// Where SMAPI writes its log on every platform the manager supports.
    ///
    /// This is deliberately a description rather than a list of paths that
    /// exist: the point is to tell a user where to look on the machine they are
    /// actually using, including when that is not the machine they are running
    /// the manager on.
    fn smapi_log_locations(&self) -> Vec<PlatformPathDto> {
        let described = self.log_reader.known_log_locations();
        if !described.is_empty() {
            return described
                .into_iter()
                .map(|(operating_system, path)| PlatformPathDto {
                    operating_system: operating_system.clone(),
                    context: format!("SMAPI log ({})", platform_label(&operating_system)),
                    path,
                })
                .collect();
        }
        default_log_locations()
    }

    pub fn get_smapi_log(&self) -> AppResult<String> {
        self.log_reader.read_log_content()
    }

    pub fn get_smapi_log_path(&self) -> PathBuf {
        self.log_reader.log_file_path()
    }
}

fn render(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn platform_label(operating_system: &str) -> &str {
    match operating_system {
        "windows" => "Windows",
        "macos" => "macOS",
        _ => "Linux",
    }
}

/// The documented log locations, used when the adapter does not describe them.
pub fn default_log_locations() -> Vec<PlatformPathDto> {
    let mut locations = Vec::new();
    for (operating_system, context, path) in [
        (
            OperatingSystem::Linux,
            "SMAPI log (Linux)",
            "~/.config/StardewValley/ErrorLogs/SMAPI-latest.txt",
        ),
        (
            OperatingSystem::Windows,
            "SMAPI log (Windows)",
            "%APPDATA%\\StardewValley\\ErrorLogs\\SMAPI-latest.txt",
        ),
        (
            OperatingSystem::MacOS,
            "SMAPI log (macOS)",
            "~/.config/StardewValley/ErrorLogs/SMAPI-latest.txt",
        ),
    ] {
        locations.push(PlatformPathDto {
            operating_system: operating_system.as_key().to_string(),
            context: context.to_string(),
            path: path.to_string(),
        });
    }
    locations
}

/// When a SMAPI log says it started ("Log started at 2026-09-13T12:34:56 UTC").
fn log_start_time(raw_log: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let line = raw_log.lines().find(|l| l.contains("Log started at "))?;
    let text = line.split("Log started at ").nth(1)?.trim();
    let text = text.trim_end_matches(" UTC").trim_end_matches('Z');
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|naive| naive.and_utc())
}

#[cfg(test)]
mod log_start_tests {
    use super::log_start_time;

    #[test]
    fn the_start_line_is_read_and_anything_else_is_unknown() {
        let log = "[12:34:56 INFO  SMAPI] Log started at 2026-09-13T12:34:56 UTC\nmore";
        assert_eq!(
            log_start_time(log).unwrap().to_rfc3339(),
            "2026-09-13T12:34:56+00:00"
        );
        assert!(log_start_time("no start here").is_none());
        assert!(log_start_time("Log started at yesterday").is_none());
    }
}
