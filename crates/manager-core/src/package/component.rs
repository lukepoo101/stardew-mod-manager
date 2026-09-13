use crate::ids::{ArtifactHash, ModUniqueId, PackageComponentId};
use crate::manifest::Manifest;
use serde::{Deserialize, Serialize};

/// A mod or content-pack component discovered within a package artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageComponent {
    pub id: PackageComponentId,
    pub artifact_hash: ArtifactHash,
    pub unique_id: ModUniqueId,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: Option<String>,
    pub relative_component_root: String,
    pub raw_manifest: String,
    pub manifest: Manifest,
}
