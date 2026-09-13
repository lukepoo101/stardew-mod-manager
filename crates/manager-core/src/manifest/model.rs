use crate::ids::ModUniqueId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModDependency {
    pub unique_id: ModUniqueId,
    pub minimum_version: Option<String>,
    pub is_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentPackFor {
    pub unique_id: ModUniqueId,
    pub minimum_version: Option<String>,
}

/// Normalized SMAPI manifest representation.
///
/// Note: Preserves only normalized semantic fields. Raw JSON source is preserved
/// separately in `PackageComponent` or document envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub unique_id: ModUniqueId,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: Option<String>,
    pub entry_dll: Option<String>,
    pub minimum_api_version: Option<String>,
    pub minimum_game_version: Option<String>,
    #[serde(default)]
    pub update_keys: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<ModDependency>,
    pub content_pack_for: Option<ContentPackFor>,
}
