//! The process-lifecycle capability behind the game launcher.
//!
//! The launch service needs three answers from the operating system: start this
//! process, is it still the same process, and stop the process this manager
//! owns. PID reuse makes the second question the hard one on every platform,
//! and the answer has the same shape everywhere: a process is the one this
//! session started only while its (pid, creation time, image) triple still
//! matches.
//!
//! Implementations must fail closed. When a backend cannot prove that a pid it
//! is asked about has died, it reports "running"; when it cannot prove that a
//! process is the one this manager started, it refuses to terminate it.

use manager_app::error::AppResult;
use manager_core::launch::LaunchSpec;

/// A process the manager started and can still identify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// The creation timestamp of this exact process incarnation, which is what
    /// makes the identity survive PID reuse.
    pub creation_time: Option<u64>,
    /// The image path recorded at launch, when the platform exposes one.
    pub image_path: Option<String>,
}

impl ProcessIdentity {
    pub fn new(pid: u32, creation_time: Option<u64>, image_path: Option<String>) -> Self {
        Self {
            pid,
            creation_time,
            image_path,
        }
    }
}

// Every method here reports through the application error type, which is
// deliberately rich; boxing it would hide the code and context callers act on.
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
