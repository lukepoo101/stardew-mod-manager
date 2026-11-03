use std::any::Any;

/// Cross-process mutual exclusion for mutations of a game installation.
///
/// This is the only remaining core port: every other external effect is owned
/// by a bounded `manager-app` port rather than a core-wide domain interface.
pub trait InstanceLock: Send + Sync {
    fn acquire_guard(&self) -> Result<Box<dyn Any + Send + Sync>, String>;

    /// Whether a different process holds the lock right now, which is what
    /// proves another process is carrying out a change. Never blocks. An
    /// implementation that cannot tell says no, so an unfinished change is
    /// offered for recovery (which takes the lock itself) rather than waited
    /// on forever.
    fn held_by_another_process(&self) -> bool {
        false
    }
}
