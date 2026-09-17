use crate::api::dto::{DiagnosticsDto, FindingDto, PlatformPathDto};
use crate::error::AppResult;
use crate::ports::logging::SessionLogPort;
use crate::ports::repositories::LaunchSessionRepository;
use manager_core::game::OperatingSystem;
use manager_core::ids::LaunchSessionId;
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

        let mut findings = Vec::new();
        if raw_log.contains("[SMAPI]    Failed:") || raw_log.contains("An error occurred") {
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
                observed_at: chrono::Utc::now().to_rfc3339(),
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

        Ok(DiagnosticsDto {
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
