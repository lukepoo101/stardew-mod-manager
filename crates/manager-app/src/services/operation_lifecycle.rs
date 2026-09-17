//! Central operation bookkeeping.
//!
//! Every runtime operation-state change and every persisted execution step goes
//! through this small helper, so the state machine is validated in one place
//! instead of being re-derived at each call site. It is deliberately boring:
//! transition, start a step, complete a step, fail a step, load steps, record
//! error metadata. Business decisions stay in the services that own them.
//!
//! Authoritative writes are never discarded here. Recording that an operation
//! entered `RecoveryRequired` is itself part of the correctness contract, so a
//! failure to persist it is a failure of the operation, not housekeeping.

use crate::error::{AppError, AppResult};
use crate::ports::repositories::OperationRepository;
use manager_core::ids::OperationId;
use manager_core::operation::{
    is_valid_transition, OperationState, OperationStep, OperationStepKind, OperationStepState,
};
use std::sync::Arc;

pub struct OperationLifecycle {
    operation_repo: Arc<dyn OperationRepository>,
}

impl OperationLifecycle {
    pub fn new(operation_repo: Arc<dyn OperationRepository>) -> Self {
        Self { operation_repo }
    }

    pub fn repository(&self) -> &Arc<dyn OperationRepository> {
        &self.operation_repo
    }

    /// Moves an operation to `target`, validating the transition first.
    ///
    /// The database enforces the same rule inside its write; validating here as
    /// well keeps the rule visible at the layer that decides the transition and
    /// gives the caller a typed conflict instead of a storage failure.
    pub fn transition(
        &self,
        operation_id: &OperationId,
        target: OperationState,
        error_code: Option<&str>,
        error_json: Option<String>,
    ) -> AppResult<()> {
        let operation = self
            .operation_repo
            .get_operation(operation_id)?
            .ok_or_else(|| {
                AppError::validation(
                    "OPERATION_NOT_FOUND",
                    format!("Operation {} not found", operation_id),
                )
            })?;

        if operation.state != target && !is_valid_transition(operation.state, target) {
            return Err(AppError::conflict(
                "INVALID_OPERATION_TRANSITION",
                "The operation cannot move to that state",
                format!(
                    "Cannot transition operation {} from {:?} to {:?}",
                    operation_id, operation.state, target
                ),
                crate::error::Recoverability::Terminal,
            ));
        }

        self.operation_repo.update_operation_state(
            operation_id,
            target,
            error_code.map(str::to_string),
            error_json,
        )
    }

    /// Records the error metadata an operation carries, without moving it.
    pub fn record_error(
        &self,
        operation_id: &OperationId,
        code: &str,
        error_json: Option<String>,
    ) -> AppResult<()> {
        self.operation_repo.update_operation_metadata(
            operation_id,
            Some(code.to_string()),
            error_json,
        )
    }

    /// Persists a step as `Running` before the side effect it names is attempted.
    pub fn start_step(
        &self,
        operation_id: &OperationId,
        step_index: u32,
        kind: OperationStepKind,
        payload_json: serde_json::Value,
    ) -> AppResult<()> {
        self.operation_repo.save_operation_step(&OperationStep {
            operation_id: *operation_id,
            step_index,
            step_kind: kind.as_str().to_string(),
            state: OperationStepState::Running,
            payload_json: payload_json.to_string(),
            started_at: Some(chrono::Utc::now()),
            completed_at: None,
            error_json: None,
        })
    }

    /// Persists a step as `Completed` once evidence confirms the effect.
    pub fn complete_step(
        &self,
        operation_id: &OperationId,
        step_index: u32,
        kind: OperationStepKind,
        payload_json: serde_json::Value,
    ) -> AppResult<()> {
        let started_at = self
            .load_steps(operation_id)?
            .into_iter()
            .find(|step| step.step_index == step_index)
            .and_then(|step| step.started_at);
        self.operation_repo.save_operation_step(&OperationStep {
            operation_id: *operation_id,
            step_index,
            step_kind: kind.as_str().to_string(),
            state: OperationStepState::Completed,
            payload_json: payload_json.to_string(),
            started_at,
            completed_at: Some(chrono::Utc::now()),
            error_json: None,
        })
    }

    /// Persists a step as `Failed` with its evidence.
    pub fn fail_step(
        &self,
        operation_id: &OperationId,
        step_index: u32,
        error_json: Option<String>,
    ) -> AppResult<()> {
        let existing = self
            .load_steps(operation_id)?
            .into_iter()
            .find(|step| step.step_index == step_index)
            .ok_or_else(|| {
                AppError::internal(
                    "OPERATION_STEP_MISSING",
                    format!("Operation {} has no step {}", operation_id, step_index),
                )
            })?;

        self.operation_repo.save_operation_step(&OperationStep {
            state: OperationStepState::Failed,
            completed_at: Some(chrono::Utc::now()),
            error_json,
            ..existing
        })
    }

    pub fn load_steps(&self, operation_id: &OperationId) -> AppResult<Vec<OperationStep>> {
        self.operation_repo.list_operation_steps(operation_id)
    }

    /// The persisted state of a named step, if the operation has crossed it.
    pub fn step_state(
        &self,
        operation_id: &OperationId,
        kind: OperationStepKind,
    ) -> AppResult<Option<OperationStepState>> {
        Ok(self
            .load_steps(operation_id)?
            .into_iter()
            .find(|step| OperationStepKind::parse(&step.step_kind) == Some(kind))
            .map(|step| step.state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_core::operation::{Operation, OperationKind, OPERATION_PLAN_SCHEMA_V2};
    use std::sync::Mutex;

    /// A minimal in-memory journal: these tests cover the lifecycle rules, not
    /// SQLite, which has its own repository tests.
    #[derive(Default)]
    struct FakeOperationRepository {
        operation: Mutex<Option<Operation>>,
        steps: Mutex<Vec<OperationStep>>,
    }

    impl OperationRepository for FakeOperationRepository {
        fn create_operation(&self, op: &Operation) -> AppResult<()> {
            *self.operation.lock().unwrap() = Some(op.clone());
            Ok(())
        }
        fn save_operation(&self, op: &Operation) -> AppResult<()> {
            *self.operation.lock().unwrap() = Some(op.clone());
            Ok(())
        }
        fn get_operation(&self, _id: &OperationId) -> AppResult<Option<Operation>> {
            Ok(self.operation.lock().unwrap().clone())
        }
        fn update_operation_state(
            &self,
            _id: &OperationId,
            state: OperationState,
            error_code: Option<String>,
            error_json: Option<String>,
        ) -> AppResult<()> {
            let mut guard = self.operation.lock().unwrap();
            let op = guard.as_mut().expect("operation");
            op.state = state;
            op.error_code = error_code;
            op.error_json = error_json;
            Ok(())
        }
        fn update_operation_metadata(
            &self,
            _id: &OperationId,
            error_code: Option<String>,
            error_json: Option<String>,
        ) -> AppResult<()> {
            let mut guard = self.operation.lock().unwrap();
            let op = guard.as_mut().expect("operation");
            op.error_code = error_code;
            op.error_json = error_json;
            Ok(())
        }
        fn list_unresolved_operations(&self) -> AppResult<Vec<Operation>> {
            Ok(Vec::new())
        }
        fn list_recent_operations(&self, _limit: usize) -> AppResult<Vec<Operation>> {
            Ok(Vec::new())
        }
        fn list_operations_for_profile(
            &self,
            _profile_id: &manager_core::ids::ProfileId,
        ) -> AppResult<Vec<Operation>> {
            Ok(Vec::new())
        }
        fn save_operation_step(&self, step: &OperationStep) -> AppResult<()> {
            let mut steps = self.steps.lock().unwrap();
            match steps
                .iter_mut()
                .find(|existing| existing.step_index == step.step_index)
            {
                Some(existing) => *existing = step.clone(),
                None => steps.push(step.clone()),
            }
            Ok(())
        }
        fn list_operation_steps(&self, _op_id: &OperationId) -> AppResult<Vec<OperationStep>> {
            Ok(self.steps.lock().unwrap().clone())
        }
        fn save_operation_resource(
            &self,
            _res: &manager_core::operation::OperationResource,
        ) -> AppResult<()> {
            Ok(())
        }
        fn list_operation_resources(
            &self,
            _op_id: &OperationId,
        ) -> AppResult<Vec<manager_core::operation::OperationResource>> {
            Ok(Vec::new())
        }
        fn list_unresolved_resources(
            &self,
            _resource_kind: manager_core::operation::ResourceKind,
            _resource_id: &str,
        ) -> AppResult<Vec<manager_core::operation::OperationResource>> {
            Ok(Vec::new())
        }
        fn save_operation_effect(
            &self,
            _effect: &manager_core::operation::OperationEffect,
        ) -> AppResult<()> {
            Ok(())
        }
        fn list_operation_effects(
            &self,
            _op_id: &OperationId,
        ) -> AppResult<Vec<manager_core::operation::OperationEffect>> {
            Ok(Vec::new())
        }
        fn list_recent_effects_for_profile(
            &self,
            _profile_id: &manager_core::ids::ProfileId,
            _limit: usize,
        ) -> AppResult<Vec<manager_core::operation::OperationEffect>> {
            Ok(Vec::new())
        }
    }

    fn prepared_operation() -> Operation {
        Operation {
            id: OperationId::new(),
            kind: OperationKind::ModInstall,
            state: OperationState::Prepared,
            game_installation_id: None,
            profile_id: None,
            expected_profile_revision: Some(1),
            plan_schema_version: OPERATION_PLAN_SCHEMA_V2,
            plan_json: "{}".to_string(),
            progress_current: None,
            progress_total: None,
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
        }
    }

    fn lifecycle() -> (OperationLifecycle, Arc<FakeOperationRepository>) {
        let repo = Arc::new(FakeOperationRepository::default());
        (OperationLifecycle::new(repo.clone()), repo)
    }

    #[test]
    fn a_prepared_operation_may_enter_running() {
        let (lifecycle, repo) = lifecycle();
        let operation = prepared_operation();
        repo.save_operation(&operation).unwrap();

        lifecycle
            .transition(&operation.id, OperationState::Running, None, None)
            .expect("Prepared -> Running is legal");
        assert_eq!(
            repo.get_operation(&operation.id).unwrap().unwrap().state,
            OperationState::Running
        );
    }

    #[test]
    fn an_illegal_transition_is_rejected_before_it_is_persisted() {
        let (lifecycle, repo) = lifecycle();
        let operation = prepared_operation();
        repo.save_operation(&operation).unwrap();

        let error = lifecycle
            .transition(&operation.id, OperationState::Succeeded, None, None)
            .expect_err("Prepared -> Succeeded is not a legal transition");
        assert_eq!(error.code, "INVALID_OPERATION_TRANSITION");
        assert_eq!(
            repo.get_operation(&operation.id).unwrap().unwrap().state,
            OperationState::Prepared
        );
    }

    #[test]
    fn steps_move_from_running_to_completed_or_failed() {
        let (lifecycle, repo) = lifecycle();
        let operation = prepared_operation();
        repo.save_operation(&operation).unwrap();

        lifecycle
            .start_step(
                &operation.id,
                4,
                OperationStepKind::PublishDeployment,
                serde_json::json!({ "mod_folder_name": "Author.Mod" }),
            )
            .unwrap();
        assert_eq!(
            lifecycle
                .step_state(&operation.id, OperationStepKind::PublishDeployment)
                .unwrap(),
            Some(OperationStepState::Running)
        );

        lifecycle
            .complete_step(
                &operation.id,
                4,
                OperationStepKind::PublishDeployment,
                serde_json::json!({ "mod_folder_name": "Author.Mod" }),
            )
            .unwrap();
        let steps = lifecycle.load_steps(&operation.id).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].state, OperationStepState::Completed);
        assert!(steps[0].completed_at.is_some());
        assert!(steps[0].started_at.is_some());
    }

    #[test]
    fn failing_a_step_keeps_its_identity_and_records_the_error() {
        let (lifecycle, repo) = lifecycle();
        let operation = prepared_operation();
        repo.save_operation(&operation).unwrap();

        lifecycle
            .start_step(
                &operation.id,
                5,
                OperationStepKind::CommitInstallDatabase,
                serde_json::json!({}),
            )
            .unwrap();
        lifecycle
            .fail_step(
                &operation.id,
                5,
                Some(r#"{"code":"COMMIT_FAILED"}"#.to_string()),
            )
            .unwrap();

        let steps = lifecycle.load_steps(&operation.id).unwrap();
        assert_eq!(steps[0].state, OperationStepState::Failed);
        assert_eq!(steps[0].step_kind, "commit_install_database");
        assert!(steps[0].error_json.is_some());
    }
}
