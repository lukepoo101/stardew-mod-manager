use crate::error::AppResult;
use manager_core::ids::ModUniqueId;
use manager_core::launch::{SessionVerificationBaseline, SessionVerificationResult};
use std::path::PathBuf;

/// A mod the running session is expected to load.
///
/// SMAPI's log reports mods by their human-readable name, so verification needs
/// the display name alongside the UniqueID and version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedMod {
    pub unique_id: ModUniqueId,
    pub name: String,
    pub version: String,
}

pub trait SessionLogPort: Send + Sync {
    fn capture_baseline(&self) -> AppResult<SessionVerificationBaseline>;
    fn verify_session(
        &self,
        baseline: &SessionVerificationBaseline,
        expected_mods: &[ExpectedMod],
    ) -> AppResult<SessionVerificationResult>;
    fn read_log_content(&self) -> AppResult<String>;
    fn log_file_path(&self) -> PathBuf;
}
