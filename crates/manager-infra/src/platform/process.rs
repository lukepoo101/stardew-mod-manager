//! The process-lifecycle capability behind the game launcher.
//!
//! The launch service needs four answers from the operating system: start this
//! process, is it still the same process, can a previously recorded process
//! still be identified after a restart, and stop the process this manager owns.
//! PID reuse makes the identity questions the hard ones on every platform, and
//! the answer has the same shape everywhere: a process is the one this session
//! started only while its (pid, creation time, image) triple still matches.
//!
//! Implementations must fail closed. When a backend cannot prove that a pid it
//! is asked about has died, it reports "running"; when it cannot prove that a
//! process is the one this manager started, it refuses to terminate it.

use manager_app::error::AppResult;
use manager_core::launch::{LaunchSpec, ProcessIdentity};

// The identity value lives in the domain because it is persisted with the
// launch session; it is re-exported here so backend implementations and their
// callers have one name for it.
pub use manager_core::launch::ProcessIdentity as TrackedProcessIdentity;

#[allow(clippy::result_large_err)]
pub trait ProcessBackend: Send + Sync {
    /// Starts a detached process and returns the identity it is tracked under.
    fn spawn(&self, spec: &LaunchSpec) -> AppResult<ProcessIdentity>;

    /// Whether the process behind a pid this session launched is still alive.
    ///
    /// A pid that is not tracked at all is not "alive" in the sense this
    /// answers; callers use unknown_pid_is_alive for that.
    fn is_alive(&self, pid: u32) -> bool;

    /// Whether any process this session started is still running.
    fn any_owned_alive(&self) -> bool;

    /// Whether a pid this session did not start is running.
    ///
    /// A bare pid is not an identity, so a backend reports true unless it can
    /// prove the process is gone.
    fn unknown_pid_is_alive(&self, pid: u32) -> bool;

    /// The identity this session tracks for a pid, if any.
    fn identity_for(&self, pid: u32) -> Option<ProcessIdentity>;

    /// Whether a previously recorded identity still identifies a live process.
    ///
    /// This is the restart case: the manager has an identity it wrote to its
    /// own database and no in-memory tracking, and it must decide whether the
    /// process is still the one it started. A backend that cannot re-establish
    /// the identity reports false, which callers must treat as "unknown" rather
    /// than as "still running".
    fn identify_recorded(&self, identity: &ProcessIdentity) -> RecordedProcessState;

    /// Terminates a process this session owns.
    fn terminate_owned(&self, pid: u32) -> AppResult<()>;

    /// Terminates every process this session owns.
    fn terminate_all_owned(&self) -> AppResult<()>;

    /// Releases tracking state for a process that has ended.
    fn forget(&self, pid: u32);

    /// Whether a game process this session did not start is running.
    ///
    /// This is what stops the manager from mutating a game directory while the
    /// user is playing, even when Steam or an earlier manager session started
    /// the game.
    fn discover_external(&self) -> bool;
}

/// What a recorded identity proves after a restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordedProcessState {
    /// The recorded process incarnation is still running.
    Running,
    /// The pid is gone, or now belongs to a different incarnation.
    Exited,
    /// The identity could not be checked, so nothing is proven either way.
    Unknown,
}
