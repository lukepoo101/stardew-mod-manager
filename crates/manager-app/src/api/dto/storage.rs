use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One item the storage cleanup found, and whether it may be removed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "CleanupItemDto.ts")]
pub struct CleanupItemDto {
    /// Stable identifier to pass back when running the cleanup.
    pub id: String,
    /// "cache", "unused_package", "operation_leftover" or "protected".
    pub category: String,
    pub label: String,
    /// Why the item is in this category, in plain words.
    pub detail: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub removable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "CleanupPreviewDto.ts")]
pub struct CleanupPreviewDto {
    pub items: Vec<CleanupItemDto>,
    #[ts(type = "number")]
    pub reclaimable_bytes: u64,
    #[ts(type = "number")]
    pub protected_bytes: u64,
    /// Set when something running right now protects items that would
    /// otherwise be removable.
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "CleanupOutcomeDto.ts")]
pub struct CleanupOutcomeDto {
    pub id: String,
    pub label: String,
    /// "removed", "failed" or "skipped".
    pub outcome: String,
    /// Why an item failed or was skipped.
    pub message: Option<String>,
    #[ts(type = "number")]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "CleanupResultDto.ts")]
pub struct CleanupResultDto {
    pub outcomes: Vec<CleanupOutcomeDto>,
    #[ts(type = "number")]
    pub reclaimed_bytes: u64,
    /// True only when every requested item was removed.
    pub complete: bool,
}
