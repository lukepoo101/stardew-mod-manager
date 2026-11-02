//! What the command line prints with `--json`, where it says more than the
//! app's own DTOs.

use super::{BootstrapDto, BulkToggleResultDto};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `cli status`: the app's bootstrap data plus what a script needs to know
/// before it changes anything.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "CliStatusDto.ts")]
pub struct CliStatusDto {
    #[serde(flatten)]
    pub bootstrap: BootstrapDto,
    /// Whether an app window is open on this data folder now.
    pub app_open: bool,
    /// The active game's folder.
    pub game_path: Option<String>,
    pub active_profile_name: Option<String>,
    /// Changes that are not finished, of every kind (see `CliOperationStatusDto`).
    pub unfinished_operations: Vec<CliOperationStatusDto>,
    /// Profiles whose duplication stopped part-way, by name.
    pub unfinished_copies: Vec<String>,
}

/// A change that has not finished, and what it means for a script.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "CliOperationStatusDto.ts")]
pub struct CliOperationStatusDto {
    pub operation_id: String,
    pub title: String,
    /// "needs_recovery": the app must recover it; changes are refused.
    /// "interrupted": the app stopped while carrying it out; it is recovered
    /// when the app next starts, and changes are refused until then.
    /// "in_progress": the open app is carrying it out right now.
    /// "open_review": an install or removal review nobody has confirmed;
    /// nothing has changed, and it never blocks a change.
    pub status: String,
    pub started_at: String,
}

/// One mod in a `cli enable`/`disable` plan.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "CliModRefDto.ts")]
pub struct CliModRefDto {
    pub unique_id: String,
    pub name: String,
    pub version: String,
}

/// `cli enable`/`disable`: what would change (or did, with `--yes`) and the
/// same warnings the app shows before it changes anything.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "CliTogglePlanDto.ts")]
pub struct CliTogglePlanDto {
    pub enable: bool,
    /// The mods named that will change.
    pub changes: Vec<CliModRefDto>,
    /// The mods named that are already in the requested state.
    pub unchanged: Vec<CliModRefDto>,
    /// Every mod that moves, including ones that share a package with a mod
    /// named, by name.
    pub affected_mods: Vec<String>,
    /// Enabled mods that will stop loading, by name (disabling).
    pub dependents: Vec<String>,
    /// Requirements left unmet (enabling).
    pub unmet_requirements: Vec<String>,
    pub applied: bool,
    /// What happened to each mod, once applied.
    pub result: Option<BulkToggleResultDto>,
}
