use std::any::Any;

/// Cross-process mutual exclusion for mutations of a game installation.
///
/// This is the only remaining core port: every other external effect is owned
/// by a bounded `manager-app` port rather than a core-wide domain interface.
pub trait InstanceLock: Send + Sync {
    fn acquire_guard(&self) -> Result<Box<dyn Any + Send + Sync>, String>;
}
