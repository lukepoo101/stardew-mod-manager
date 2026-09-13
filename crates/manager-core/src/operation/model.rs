use crate::ids::{GameInstallationId, OperationId, ProfileId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    SmapiSetup,
    ModInstall,
    ModRemove,
    GameLaunch,
    ProfileCreate,
    ProfileDelete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Draft,
    Prepared,
    Running,
    Committing,
    Succeeded,
    Failed,
    CancellationRequested,
    Cancelling,
    Cancelled,
    RollingBack,
    RolledBack,
    RecoveryRequired,
}

impl OperationState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::RolledBack
        )
    }

    pub fn requires_recovery(&self) -> bool {
        matches!(self, Self::RecoveryRequired)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStepState {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    GameInstallation,
    Profile,
    Artifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    Read,
    Write,
}

/// A durable operation representing an orchestrated task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub id: OperationId,
    pub kind: OperationKind,
    pub state: OperationState,
    pub game_installation_id: Option<GameInstallationId>,
    pub profile_id: Option<ProfileId>,
    pub expected_profile_revision: Option<u64>,
    pub plan_schema_version: u32,
    pub plan_json: String,
    pub progress_current: Option<u32>,
    pub progress_total: Option<u32>,
    pub error_code: Option<String>,
    pub error_json: Option<String>,
    pub cancellation_requested: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// An individual step within an operation for fine-grained progress and resumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationStep {
    pub operation_id: OperationId,
    pub step_index: u32,
    pub step_kind: String,
    pub state: OperationStepState,
    pub payload_json: String,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_json: Option<String>,
}

/// Declared resource scope for conflict detection and resource-scoped recovery.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationResource {
    pub operation_id: OperationId,
    pub resource_kind: ResourceKind,
    pub resource_id: String,
    pub access_mode: AccessMode,
}

/// A semantic state mutation effect recorded in the audit journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEffect {
    pub id: String,
    pub operation_id: OperationId,
    pub profile_id: Option<ProfileId>,
    pub entity_type: String,
    pub entity_id: String,
    pub change_kind: String,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub occurred_at: DateTime<Utc>,
}
