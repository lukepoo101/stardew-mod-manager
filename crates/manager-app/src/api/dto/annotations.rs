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
    /// A source link the user added themselves. It is marked as theirs and
    /// never replaces how the package was actually acquired.
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_added_at: Option<String>,
}
