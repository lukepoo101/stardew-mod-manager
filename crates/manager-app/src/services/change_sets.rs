//! Change sets: one journaled operation over a change made of several other
//! operations, such as restoring a restore point or putting a profile in step
//! with its group reference.
//!
//! Each part is itself a normal journaled operation and recovers on its own.
//! The change set records which parts finished. If the app stops part-way,
//! startup recovery marks the change set interrupted, and it is listed until
//! the user finishes it (the same change again, worked out from the profile
//! as it is then) or puts it aside. Nothing is guessed or redone
//! automatically.

use crate::api::dto::UnfinishedChangeDto;
use crate::error::{AppError, AppResult};
use crate::ports::repositories::{OperationRepository, PreferencesRepository};
use crate::services::operation_lifecycle::OperationLifecycle;
use manager_core::ids::{OperationId, ProfileId};
use manager_core::operation::{
    Operation, OperationKind, OperationState, OperationStep, OperationStepKind, OperationStepState,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;

/// Set by startup recovery on a change set the app stopped in the middle of.
pub const CHANGE_SET_INTERRUPTED: &str = "CHANGE_SET_INTERRUPTED";
/// Set when a change set finished with some parts failing.
pub const CHANGE_SET_INCOMPLETE: &str = "CHANGE_SET_INCOMPLETE";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Plan {
    title: String,
    /// What finishing it means: "restore_point" or "reference".
    resume_kind: String,
    /// For a restore point, its id; for a reference, empty.
    #[serde(default)]
    resume_target: String,
    /// For a restore, the point saved before it, to undo it.
    #[serde(default)]
    undo_point_id: Option<String>,
}

fn dismissed_key(id: &OperationId) -> String {
    format!("change_set_put_aside:{id}")
}

pub struct ChangeSets {
    operation_repo: Arc<dyn OperationRepository>,
    preferences: Arc<dyn PreferencesRepository>,
}

impl ChangeSets {
    pub fn new(
        operation_repo: Arc<dyn OperationRepository>,
        preferences: Arc<dyn PreferencesRepository>,
    ) -> Self {
        Self {
            operation_repo,
            preferences,
        }
    }

    /// Starts a change set with its parts listed, all pending.
    pub fn begin(
        &self,
        profile_id: &ProfileId,
        title: &str,
        resume_kind: &str,
        resume_target: &str,
        undo_point_id: Option<&str>,
        parts: &[String],
    ) -> AppResult<OperationId> {
        if !matches!(resume_kind, "restore_point" | "reference") {
            return Err(AppError::validation(
                "CHANGE_SET_KIND",
                "Unknown kind of change",
            ));
        }
        let now = chrono::Utc::now();
        let id = OperationId::new();
        let plan = Plan {
            title: title.chars().take(120).collect(),
            resume_kind: resume_kind.to_string(),
            resume_target: resume_target.to_string(),
            undo_point_id: undo_point_id.map(str::to_string),
        };
        self.operation_repo.create_operation(&Operation {
            id,
            kind: OperationKind::ProfileChangeSet,
            state: OperationState::Committing,
            game_installation_id: None,
            profile_id: Some(*profile_id),
            expected_profile_revision: None,
            plan_schema_version: 1,
            plan_json: serde_json::to_string(&plan)
                .map_err(|e| AppError::internal("Could not record the change", e.to_string()))?,
            progress_current: Some(0),
            progress_total: Some(parts.len() as u32),
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: now,
            updated_at: now,
            completed_at: None,
        })?;
        let lifecycle = OperationLifecycle::new(self.operation_repo.clone());
        for (index, part) in parts.iter().enumerate() {
            lifecycle.plan_step(
                &id,
                index as u32,
                OperationStepKind::ChangeSetPart,
                serde_json::json!({ "label": part }),
            )?;
        }
        Ok(id)
    }

    /// Records how one part went.
    pub fn part_done(&self, id: &OperationId, index: u32, error: Option<&str>) -> AppResult<()> {
        let lifecycle = OperationLifecycle::new(self.operation_repo.clone());
        let step = lifecycle
            .load_steps(id)?
            .into_iter()
            .find(|s| s.step_index == index)
            .ok_or_else(|| AppError::validation("CHANGE_SET_PART", "No such part"))?;
        match error {
            Some(error) => lifecycle.fail_step(id, index, Some(error.to_string())),
            None => lifecycle.complete_step(
                id,
                index,
                OperationStepKind::ChangeSetPart,
                serde_json::from_str(&step.payload_json).unwrap_or_default(),
            ),
        }
    }

    /// Ends a change set: succeeded, or failed naming the parts that did not
    /// work. The parts that worked stay done.
    pub fn finish(&self, id: &OperationId, failures: &[String]) -> AppResult<()> {
        let lifecycle = OperationLifecycle::new(self.operation_repo.clone());
        if failures.is_empty() {
            lifecycle.transition(id, OperationState::Succeeded, None, None)
        } else {
            lifecycle.transition(
                id,
                OperationState::Failed,
                Some(CHANGE_SET_INCOMPLETE),
                Some(failures.join("; ")),
            )
        }
    }

    /// Change sets of the profile that were interrupted and not put aside,
    /// newest first.
    pub fn unfinished(&self, profile_id: &ProfileId) -> AppResult<Vec<UnfinishedChangeDto>> {
        let mut out = Vec::new();
        for op in self
            .operation_repo
            .list_operations_for_profile(profile_id)?
        {
            if op.kind != OperationKind::ProfileChangeSet
                || op.error_code.as_deref() != Some(CHANGE_SET_INTERRUPTED)
                || self
                    .preferences
                    .get_preference(&dismissed_key(&op.id))?
                    .is_some()
            {
                continue;
            }
            let Ok(plan) = serde_json::from_str::<Plan>(&op.plan_json) else {
                continue;
            };
            let steps = self.operation_repo.list_operation_steps(&op.id)?;
            let label = |s: &OperationStep| {
                serde_json::from_str::<serde_json::Value>(&s.payload_json)
                    .ok()
                    .and_then(|v| v.get("label").and_then(|l| l.as_str()).map(str::to_string))
                    .unwrap_or_default()
            };
            out.push(UnfinishedChangeDto {
                operation_id: op.id.to_string(),
                title: plan.title,
                resume_kind: plan.resume_kind,
                resume_target: plan.resume_target,
                undo_point_id: plan.undo_point_id,
                started_at: op.created_at.to_rfc3339(),
                parts_done: steps
                    .iter()
                    .filter(|s| s.state == OperationStepState::Completed)
                    .map(label)
                    .collect(),
                parts_left: steps
                    .iter()
                    .filter(|s| s.state != OperationStepState::Completed)
                    .map(label)
                    .collect(),
            });
        }
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(out)
    }

    /// Stops listing an interrupted change set; its record stays in Activity.
    pub fn put_aside(&self, operation_id: &str) -> AppResult<()> {
        let id = OperationId::from_str(operation_id)
            .map_err(|_| AppError::validation("OPERATION_ID", "Not an operation id"))?;
        self.preferences.set_preference(&dismissed_key(&id), "1")
    }
}
