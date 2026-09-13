use crate::ids::{ArtifactHash, DeploymentId, PackageComponentId, ProfileComponentId, ProfileId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentState {
    Present,
    Disabled,
    Missing,
    ExternallyModified,
    Quarantined,
}

/// An artifact physically materialized into a profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileDeployment {
    pub id: DeploymentId,
    pub profile_id: ProfileId,
    pub artifact_hash: ArtifactHash,
    pub root_relative_path: String,
    pub installed_at: DateTime<Utc>,
    pub state: DeploymentState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstalledReason {
    Direct,
    Dependency,
    BundleCompanion,
}

/// A package component participating in a profile through a deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileComponent {
    pub id: ProfileComponentId,
    pub profile_id: ProfileId,
    pub deployment_id: DeploymentId,
    pub package_component_id: PackageComponentId,
    pub enabled: bool,
    pub installed_reason: InstalledReason,
}
