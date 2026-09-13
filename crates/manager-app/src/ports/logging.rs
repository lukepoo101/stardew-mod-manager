use crate::error::AppResult;
use manager_core::ids::ModUniqueId;
use manager_core::launch::{SessionVerificationBaseline, SessionVerificationResult};
use std::path::PathBuf;

pub trait SessionLogPort: Send + Sync {
    fn capture_baseline(&self) -> AppResult<SessionVerificationBaseline>;
    fn verify_session(
        &self,
        baseline: &SessionVerificationBaseline,
        expected_mods: &[(ModUniqueId, String)],
    ) -> AppResult<SessionVerificationResult>;
    fn read_log_content(&self) -> AppResult<String>;
    fn log_file_path(&self) -> PathBuf;
}
