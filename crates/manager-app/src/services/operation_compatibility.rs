//! Historical compatibility for operations migrated from the pre-architecture
//! runtime.
//!
//! Migration 0007 keeps the old operation journal and rewrites what it can, but
//! those plans were written by a runtime that stored a different shape: they
//! carry a `setup_id` rather than the modern commit payload. This module
//! reconciles them using the old filesystem-first contract.
//!
//! This is persisted-data compatibility, not a second runtime architecture: it
//! only ever reads rows that already exist, and no new operation is ever created
//! through it.

use crate::error::{AppError, AppResult};
use manager_core::deployment::DeploymentState;
use manager_core::ids::ProfileComponentId;
use manager_core::operation::{Operation, OperationEffect, OperationState};
use manager_core::ports::InstanceLock;
use std::str::FromStr;

use crate::ports::repositories::RemovalCommit;
use crate::services::operations::OperationsService;

/// Marks a migrated journal entry that still needs reconciliation.
pub(crate) const LEGACY_OPERATION_REQUIRES_RECONCILIATION: &str =
    "LEGACY_OPERATION_REQUIRES_RECONCILIATION";

const LEGACY_INSTALL_RECONCILED: &str = "LEGACY_INSTALL_RECONCILED";
const LEGACY_REMOVAL_RECONCILED: &str = "LEGACY_REMOVAL_RECONCILED";
const LEGACY_REMOVAL_ALREADY_COMMITTED: &str = "LEGACY_REMOVAL_ALREADY_COMMITTED";
const LEGACY_REMOVAL_NOT_PUBLISHED: &str = "LEGACY_REMOVAL_NOT_PUBLISHED";

impl OperationsService {
    /// Reconciles one migrated historical operation.
    pub(crate) fn reconcile_migrated_operation(&self, op: &Operation) -> AppResult<()> {
        match op.kind {
            manager_core::operation::OperationKind::ModInstall => {
                self.reconcile_migrated_install(op)
            }
            manager_core::operation::OperationKind::ModRemove => {
                self.reconcile_migrated_removal(op)
            }
            _ => Ok(()),
        }
    }

    /// The old runtime could have published the folder before its database
    /// transaction committed. The safe compatibility outcome is to quarantine
    /// that unowned folder and finish the operation as failed; this restores the
    /// invariant that every live deployment has a database owner.
    fn reconcile_migrated_install(&self, op: &Operation) -> AppResult<()> {
        let profile_id = op.profile_id.ok_or_else(|| {
            AppError::internal(
                "Migrated install recovery lacks profile ID",
                op.id.to_string(),
            )
        })?;
        let plan: serde_json::Value = serde_json::from_str(&op.plan_json)
            .map_err(|e| AppError::internal("Corrupted migrated install plan", e.to_string()))?;
        let folder = plan
            .get("mod_folder_name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                AppError::internal(
                    "Migrated install plan lacks deployment path",
                    op.id.to_string(),
                )
            })?;
        manager_core::install::validate_relative_path(folder)
            .map_err(|e| AppError::internal("Invalid migrated install path", e))?;

        if self.deployment.deployment_exists(&profile_id, folder)? {
            if let Err(error) = self
                .deployment
                .quarantine_deployment(&profile_id, &op.id, folder)
            {
                return self.retain_migrated_recovery(
                    op,
                    "LEGACY_INSTALL_RECONCILIATION_FAILED",
                    &error.to_string(),
                );
            }
        }

        self.lifecycle.transition(
            &op.id,
            OperationState::Failed,
            Some(LEGACY_INSTALL_RECONCILED),
            Some(
                "Migrated installation evidence was removed from the live profile; retry with a fresh plan"
                    .to_string(),
            ),
        )
    }

    /// Removes a migrated deployment whose filesystem half already happened.
    fn reconcile_migrated_removal(&self, op: &Operation) -> AppResult<()> {
        let profile_id = op.profile_id.ok_or_else(|| {
            AppError::internal(
                "Migrated removal recovery lacks profile ID",
                op.id.to_string(),
            )
        })?;
        let plan: serde_json::Value = serde_json::from_str(&op.plan_json)
            .map_err(|e| AppError::internal("Corrupted migrated removal plan", e.to_string()))?;
        let folder = plan
            .get("deployment_rel_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                AppError::internal(
                    "Migrated removal plan lacks deployment path",
                    op.id.to_string(),
                )
            })?;
        manager_core::install::validate_relative_path(folder)
            .map_err(|e| AppError::internal("Invalid migrated removal path", e))?;

        let source_exists = self.deployment.deployment_exists(&profile_id, folder)?;
        let recovery_exists =
            self.deployment
                .recovery_deployment_exists(&profile_id, &op.id, folder)?;
        if source_exists && recovery_exists {
            return self.retain_migrated_recovery(
                op,
                "LEGACY_REMOVAL_CONFLICT",
                "Both the live and recovery deployment exist; manual inspection is required",
            );
        }
        if source_exists || !recovery_exists {
            if source_exists {
                return self.lifecycle.transition(
                    &op.id,
                    OperationState::Failed,
                    Some(LEGACY_REMOVAL_NOT_PUBLISHED),
                    Some(
                        "Migrated removal did not move the deployment; it remains installed"
                            .to_string(),
                    ),
                );
            }
            return self.retain_migrated_recovery(
                op,
                "LEGACY_REMOVAL_EVIDENCE_MISSING",
                "Migrated removal evidence is missing from both live and recovery storage",
            );
        }

        let ids = plan
            .get("bundle_mod_ids")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|ids| !ids.is_empty())
            .unwrap_or_else(|| {
                plan.get("installed_mod_id")
                    .and_then(|value| value.as_str())
                    .map(|id| vec![id.to_string()])
                    .unwrap_or_default()
            });
        let component_ids = ids
            .iter()
            .map(|id| {
                ProfileComponentId::from_str(id)
                    .map_err(|e| AppError::internal("Invalid migrated component ID", e.to_string()))
            })
            .collect::<AppResult<Vec<_>>>()?;
        let components = self.deployment_repo.list_profile_components(&profile_id)?;
        let selected = components
            .into_iter()
            .filter(|component| component_ids.contains(&component.id))
            .collect::<Vec<_>>();

        // If the database commit already happened before the old process wrote
        // its final state, the recovery folder is orphaned but the operation is
        // still complete from the application's perspective.
        if selected.is_empty() {
            let deployments = self
                .deployment_repo
                .list_deployments_for_profile(&profile_id)?;
            if deployments.iter().any(|deployment| {
                deployment.root_relative_path == folder
                    && deployment.state != DeploymentState::Quarantined
            }) {
                return self.retain_migrated_recovery(
                    op,
                    "LEGACY_REMOVAL_DATABASE_STATE_UNRECONCILED",
                    "Migrated removal has no matching profile components, but its deployment is not marked quarantined",
                );
            }
            return self.lifecycle.transition(
                &op.id,
                OperationState::Succeeded,
                Some(LEGACY_REMOVAL_ALREADY_COMMITTED),
                Some("Migrated removal database state was already committed".to_string()),
            );
        }

        let deployment_id = selected[0].deployment_id;
        if selected
            .iter()
            .any(|component| component.deployment_id != deployment_id)
        {
            return self.retain_migrated_recovery(
                op,
                "LEGACY_REMOVAL_MULTIPLE_DEPLOYMENTS",
                "Migrated removal references components from multiple deployments",
            );
        }
        let profile = self
            .profile_repo
            .get_profile(&profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        let removed_profile_component_ids = selected.iter().map(|component| component.id).collect();

        // The atomic mutation store deliberately only accepts commits from its
        // committing boundary. A migrated journal starts at recovery_required,
        // so enter that boundary explicitly before applying the old removal
        // transaction.
        self.lifecycle.transition(
            &op.id,
            OperationState::Committing,
            Some("LEGACY_REMOVAL_COMMITTING"),
            None,
        )?;
        if let Err(error) = self.mutation_store.commit_removal(RemovalCommit {
            operation_id: op.id,
            profile_id,
            expected_profile_revision: profile.revision,
            deployment_id,
            removed_profile_component_ids,
            effects: Vec::<OperationEffect>::new(),
            commit_step: crate::ports::repositories::CommitStep {
                index: manager_core::operation::REMOVAL_STEP_COMMIT_DATABASE,
                kind: manager_core::operation::OperationStepKind::CommitRemovalDatabase
                    .as_str()
                    .to_string(),
                payload_json: serde_json::json!({ "deployment_rel_path": folder }).to_string(),
            },
        }) {
            return self.retain_migrated_recovery(
                op,
                "LEGACY_REMOVAL_DATABASE_RECONCILIATION_FAILED",
                &error.to_string(),
            );
        }
        self.lifecycle.record_error(
            &op.id,
            LEGACY_REMOVAL_RECONCILED,
            Some("Migrated removal filesystem and database state were reconciled".to_string()),
        )
    }

    /// Keeps the operation in recovery with its evidence intact.
    fn retain_migrated_recovery(&self, op: &Operation, code: &str, message: &str) -> AppResult<()> {
        self.lifecycle.transition(
            &op.id,
            OperationState::RecoveryRequired,
            Some(code),
            Some(message.to_string()),
        )?;
        self.lifecycle
            .record_error(&op.id, code, Some(message.to_string()))?;
        // This is a recovery situation, not an unexpected implementation
        // failure: the operation is unresolved, a human has to reconcile it, and
        // the frontend needs the operation id to route to it.
        Err(
            AppError::recovery_required(code, "Historical recovery reconciliation failed", op.id)
                .with_details(format!("{}: {}", code, message)),
        )
    }
}

/// Keeps the instance-lock import meaningful for this module's callers.
#[allow(dead_code)]
fn _instance_lock_is_owned_by_the_service(_lock: &dyn InstanceLock) {}
