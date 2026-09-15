use crate::api::dto::{DiagnosticsDto, FindingDto};
use crate::error::AppResult;
use crate::ports::logging::SessionLogPort;
use crate::ports::repositories::LaunchSessionRepository;
use manager_core::ids::LaunchSessionId;
use std::path::PathBuf;
use std::sync::Arc;

pub struct DiagnosticsService {
    session_repo: Arc<dyn LaunchSessionRepository>,
    log_reader: Arc<dyn SessionLogPort>,
}

impl DiagnosticsService {
    pub fn new(
        session_repo: Arc<dyn LaunchSessionRepository>,
        log_reader: Arc<dyn SessionLogPort>,
    ) -> Self {
        Self {
            session_repo,
            log_reader,
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

        Ok(DiagnosticsDto {
            session_id: session_id_str,
            session_state: session_state_str,
            findings,
            raw_log,
            log_file_path: log_path,
        })
    }

    pub fn get_smapi_log(&self) -> AppResult<String> {
        self.log_reader.read_log_content()
    }

    pub fn get_smapi_log_path(&self) -> PathBuf {
        self.log_reader.log_file_path()
    }
}
