use crate::error::AppResult;
use manager_core::launch::{LaunchSpec, ProcessIdentity};

/// Narrow game launching capability for starting and monitoring Stardew Valley sessions.
pub trait GameLauncherPort: Send + Sync {
    /// Starts the game and returns the identity the session is tracked under.
    ///
    /// The identity is returned rather than a bare pid because a pid is not
    /// proof of identity once the manager restarts, and the caller persists it
    /// for exactly that case.
    fn launch_game(&self, spec: &LaunchSpec) -> AppResult<ProcessIdentity>;

    fn is_game_running(&self, pid: Option<u32>) -> bool;

    /// Whether a previously recorded identity still identifies a live process.
    fn identify_recorded(&self, identity: &ProcessIdentity) -> RecordedProcessState;

    fn terminate_game(&self, pid: Option<u32>) -> AppResult<()>;
}

/// What a recorded identity proves after the manager restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordedProcessState {
    /// The recorded process incarnation is still running.
    Running,
    /// The pid is gone, or now belongs to a different incarnation.
    Exited,
    /// The identity could not be checked, so nothing is proven either way.
    Unknown,
}
