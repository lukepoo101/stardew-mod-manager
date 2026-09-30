use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The user's own notes about a mod. Keyed by UniqueID, so they follow the mod
/// into every profile, and never change what is installed or loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModAnnotationDto.ts")]
pub struct ModAnnotationDto {
    pub unique_id: String,
    pub favourite: bool,
    pub tags: Vec<String>,
    pub note: String,
}
