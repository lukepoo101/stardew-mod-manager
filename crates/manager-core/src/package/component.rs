use crate::ids::{derive_uuid, ArtifactHash, ModUniqueId, PackageComponentId};
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

impl PackageComponent {
    /// Stable identity for a component contained in immutable artifact bytes.
    /// Profile participation remains identified separately by ProfileComponentId.
    pub fn canonical_id(
        artifact_hash: &ArtifactHash,
        relative_root: &str,
        unique_id: &ModUniqueId,
    ) -> PackageComponentId {
        PackageComponentId::from(derive_uuid(&format!(
            "package-component:{}:{}:{}",
            artifact_hash.as_str(),
            relative_root,
            unique_id.as_str()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::PackageComponent;
    use crate::ids::{ArtifactHash, ModUniqueId};

    #[test]
    fn canonical_id_reuses_immutable_component_identity() {
        let artifact = ArtifactHash::new("a".repeat(64));
        let unique_id = ModUniqueId::new("Example.Mod");
        let first = PackageComponent::canonical_id(&artifact, "", &unique_id);
        let second = PackageComponent::canonical_id(&artifact, "", &unique_id);
        let other_root = PackageComponent::canonical_id(&artifact, "ContentPack", &unique_id);
        assert_eq!(first, second);
        assert_ne!(first, other_root);
    }
}
