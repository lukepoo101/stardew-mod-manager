use crate::ids::FindingId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Critical,
    Error,
    Warning,
    Info,
    Recommendation,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    Dependency,
    Runtime,
    Recovery,
    Integrity,
    Verification,
    Configuration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingAction {
    pub label: String,
    pub action_type: String,
    pub payload: Option<String>,
}

/// An observed health or diagnostic finding about the system, profile, or game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: FindingId,
    pub fingerprint: String,
    pub code: String,
    pub severity: FindingSeverity,
    pub category: FindingCategory,
    pub title: String,
    pub summary: String,
    pub affected_entities: Vec<String>,
    pub evidence: Vec<String>,
    pub actions: Vec<FindingAction>,
    pub observed_at: DateTime<Utc>,
}
