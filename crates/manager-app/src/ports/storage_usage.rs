use manager_core::ids::ProfileId;

/// Bytes on disk, or None when the location could not be read.
pub type Size = Option<u64>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileUsage {
    pub live: Size,
    pub disabled: Size,
    /// Staging and recovery folders of operations.
    pub operations: Size,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaUsage {
    pub packages: Size,
    pub installer_cache: Size,
    pub save_backups: Size,
    pub trash: Size,
}

/// Measures the manager's storage. Read only; links are not followed.
pub trait StorageUsagePort: Send + Sync {
    fn profile(&self, profile_id: &ProfileId) -> ProfileUsage;
    fn areas(&self) -> AreaUsage;
}
