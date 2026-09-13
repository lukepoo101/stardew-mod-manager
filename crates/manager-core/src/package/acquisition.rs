use crate::ids::{AcquisitionId, ArtifactHash};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionSource {
    LocalFile,
    DirectUrl,
    Provider,
    ManualReference,
}

/// Recorded provenance describing how a package artifact entered the application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acquisition {
    pub id: AcquisitionId,
    pub artifact_hash: ArtifactHash,
    pub source: AcquisitionSource,
    pub original_filename: String,
    pub acquired_at: DateTime<Utc>,
    pub expected_hash: Option<ArtifactHash>,
    pub source_metadata: Option<String>,
}
