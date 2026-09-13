use crate::ids::ArtifactHash;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Immutable source bytes retained in content-addressed storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageArtifact {
    pub hash: ArtifactHash,
    pub byte_size: u64,
    pub storage_relative_path: String,
    pub first_seen_at: DateTime<Utc>,
}
