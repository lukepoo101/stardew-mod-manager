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

/// Copies of SMAPI logs kept per game session, so an earlier session's log
/// can still be read after SMAPI has overwritten its own file.
pub trait SessionLogArchivePort: Send + Sync {
    fn save(&self, session_id: &str, content: &str) -> AppResult<()>;
    fn load(&self, session_id: &str) -> AppResult<Option<String>>;
    /// Keeps only the `keep` most recent copies.
    fn prune(&self, keep: usize) -> AppResult<()>;
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

    /// Whether a log is readable at the resolved location.
    ///
    /// The application layer must not stat the filesystem itself, so the
    /// question is asked through the port that owns the location.
    fn log_is_available(&self) -> bool;

    /// Where every platform writes this log, for diagnostics.
    ///
    /// A user reporting a problem may be reading the manager on a different
    /// machine than the one running the game, so the answer is a description
    /// rather than only the host's own path.
    fn known_log_locations(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}
