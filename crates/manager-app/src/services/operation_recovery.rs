//! Restart recovery for the operation engine.
//!
//! Recovery is routed by the operation's provenance, not by one heuristic:
//!
//! * migrated historical operations (rewritten by migration 0007) are
//!   reconciled by the historical compatibility path;
//! * plan-schema-v1 operations use the conservative plan-v1 compatibility path,
//!   because a v1 journal never recorded every boundary it crossed;
//! * plan-schema-v2 operations use the deterministic step engine, which decides
//!   from persisted steps plus concrete filesystem and database evidence.
//!
//! The rule that shapes every branch: an evidence query that fails is not
//! evidence of absence. `Ok(true)` is present, `Ok(false)` is absent, and an
//! error means the system cannot tell - which is precisely when an operation
//! needs a human instead of a guess.

use crate::error::{AppError, AppResult};
use crate::ports::repositories::RemovalCommit;
use manager_core::ids::{DeploymentId, ProfileId};
use manager_core::operation::{
    Operation, OperationKind, OperationState, OperationStepKind, INSTALL_STEP_COMMIT_DATABASE,
    INSTALL_STEP_PUBLISH_DEPLOYMENT, INSTALL_STEP_QUARANTINE_PUBLISHED, OPERATION_PLAN_SCHEMA_V1,
    OPERATION_PLAN_SCHEMA_V2, REMOVAL_STEP_COMMIT_DATABASE, REMOVAL_STEP_QUARANTINE_DEPLOYMENT,
    REMOVAL_STEP_RESTORE_QUARANTINED, SMAPI_STEP_INSTALL_FILES, SMAPI_STEP_PERSIST_STATE,
};
use manager_core::profile::Profile;
use manager_core::smapi::SmapiObservation;
use std::str::FromStr;

use crate::services::operation_compatibility::LEGACY_OPERATION_REQUIRES_RECONCILIATION;
use crate::services::operations::{OperationsService, RemovalPlan};

pub(crate) const SMAPI_RECOVERY_VERSION_UNKNOWN: &str = "SMAPI_RECOVERY_VERSION_UNKNOWN";
pub(crate) const RECOVERED_TO_TERMINAL_STATE: &str = "RECOVERED_TO_TERMINAL_STATE";
pub(crate) const RECONCILIATION_REQUIRED: &str = "RECONCILIATION_REQUIRED";
pub(crate) const PREVIEW_EXPIRED: &str = "PREVIEW_EXPIRED";

/// Whether reconciling an operation can touch files another process may own.
pub(crate) fn recovery_requires_instance_guard(profile_scoped: bool, kind: OperationKind) -> bool {
    profile_scoped || kind == OperationKind::SmapiSetup
}

/// A SMAPI installation whose version cannot be established is not recovered.
pub(crate) fn smapi_recovery_failure(
    observation: &SmapiObservation,
) -> Option<(&'static str, &'static str)> {
    if observation.is_present && observation.observed_version.is_none() {
        Some((
            SMAPI_RECOVERY_VERSION_UNKNOWN,
            "SMAPI installation evidence is present, but its version could not be determined; repair or rerun setup",
        ))
    } else {
        None
    }
}

impl OperationsService {
    /// Startup reconciliation.
    ///
    /// Independent operations are reconciled individually and a single
    /// ambiguous operation never stops the application from opening: its own
    /// evidence is persisted on the operation, and the bootstrap read model
    /// keeps reporting the unresolved state. Only a failure that prevents
    /// recovery processing itself - such as being unable to read the operations
    /// table - is fatal here.
    pub fn recover_on_startup(&self) -> AppResult<()> {
        // Reading the journal is the one step whose failure is fatal: without it
        // the application cannot know whether running is safe.
        let unresolved = self.operation_repo.list_unresolved_operations()?;
        let needs_instance_guard = unresolved
            .iter()
            .any(|op| recovery_requires_instance_guard(op.profile_id.is_some(), op.kind));

        // Both of these are environmental rather than infrastructure failures.
        // Deferring is the safe answer: the operations stay unresolved, the
        // bootstrap read model keeps reporting them, and the user can retry.
        if needs_instance_guard && self.launcher.is_game_running(None) {
            return Ok(());
        }
        let _mutation_guard = if needs_instance_guard {
            match self.instance_lock.acquire_guard() {
                Ok(guard) => Some(guard),
                Err(_) => return Ok(()),
            }
        } else {
            None
        };

        for op in unresolved {
            if let Err(error) = self.reconcile_operation(&op) {
                // The failure has already been recorded on this operation, so
                // the loop continues and the application still opens.
                eprintln!("operation {} could not be reconciled: {}", op.id, error);
            }
        }
        Ok(())
    }

    /// Explicit user-triggered reconciliation.
    ///
    /// Every unresolved operation is attempted; the first failure is reported so
    /// the frontend can route the user to the operation that needs attention.
    pub fn retry_recovery(&self) -> AppResult<()> {
        let unresolved = self.operation_repo.list_unresolved_operations()?;
        let needs_instance_guard = unresolved
            .iter()
            .any(|op| recovery_requires_instance_guard(op.profile_id.is_some(), op.kind));
        let _mutation_guard = if needs_instance_guard {
            Some(
                self.instance_lock
                    .acquire_guard()
                    .map_err(AppError::instance_locked)?,
            )
        } else {
            None
        };
        if needs_instance_guard && self.launcher.is_game_running(None) {
            return Err(AppError::game_running(
                "Stop Stardew Valley before reconciling managed files",
            ));
        }

        let mut first_error = None;
        for op in unresolved {
            if let Err(error) = self.reconcile_operation(&op) {
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Routes one unresolved operation to the reconciler that owns its
    /// provenance.
    pub(crate) fn reconcile_operation(&self, op: &Operation) -> AppResult<()> {
        // Recovery is a live mutation, so it takes the same in-process claims
        // the operation would hold while executing. Without this, an explicit
        // retry could reconcile an operation that a command in this process is
        // still running.
        let claims = self.operation_claims(op)?;
        let _resource_lease = if claims.is_empty() {
            None
        } else {
            Some(self.resources.try_acquire(&claims)?)
        };

        // A preview that never entered the mutation lifecycle has no live side
        // effect to reconcile, whatever its plan schema says.
        if matches!(op.state, OperationState::Draft | OperationState::Prepared) {
            if let Some(pid) = op.profile_id {
                let _ = self.staging.clean_staging_dir(&pid, &op.id);
            }
            return self.lifecycle.transition(
                &op.id,
                OperationState::Cancelled,
                Some(PREVIEW_EXPIRED),
                Some("Uncommitted preview was cancelled during startup recovery".to_string()),
            );
        }

        if op.error_code.as_deref() == Some(LEGACY_OPERATION_REQUIRES_RECONCILIATION)
            && matches!(
                op.kind,
                OperationKind::ModInstall | OperationKind::ModRemove
            )
        {
            return self.reconcile_migrated_operation(op);
        }

        match op.plan_schema_version {
            OPERATION_PLAN_SCHEMA_V1 => self.reconcile_v1_operation(op),
            OPERATION_PLAN_SCHEMA_V2 => self.recover_v2_operation(op),
            version => Err(AppError::new(
                "UNSUPPORTED_OPERATION_PLAN_SCHEMA",
                crate::error::AppErrorCategory::Storage,
                "Operation recovery is not supported for this plan schema",
            )
            .with_details(format!(
                "operation {} was written with plan schema version {}",
                op.id, version
            ))
            .with_operation_id(op.id)),
        }
    }

    // -----------------------------------------------------------------------
    // Plan-v1 compatibility
    // -----------------------------------------------------------------------

    /// Conservative reconciliation for operations created before the durable
    /// step engine existed.
    ///
    /// These operations cannot promise that every side effect was recorded, so
    /// they are reconciled against the coarse plan plus filesystem evidence
    /// rather than against persisted steps.
    fn reconcile_v1_operation(&self, op: &Operation) -> AppResult<()> {
        if op.state.requires_recovery() && op.kind == OperationKind::SmapiSetup {
            return self.reconcile_v1_smapi(op);
        }
        if op.state.requires_recovery()
            && op.profile_id.is_some()
            && matches!(
                op.kind,
                OperationKind::ModInstall | OperationKind::ModRemove
            )
        {
            return self.reconcile_v1_profile_operation(op);
        }
        if op.state.requires_recovery()
            || matches!(
                op.state,
                OperationState::Running
                    | OperationState::Committing
                    | OperationState::RollingBack
                    | OperationState::Cancelling
            )
        {
            // Files may already have been published or quarantined. Preserve all
            // evidence until filesystem/database reconciliation can prove an outcome.
            return self.lifecycle.transition(
                &op.id,
                OperationState::RecoveryRequired,
                Some(RECONCILIATION_REQUIRED),
                Some("Interrupted operation requires filesystem reconciliation; recovery files were preserved".to_string()),
            );
        }
        Ok(())
    }

    fn reconcile_v1_smapi(&self, op: &Operation) -> AppResult<()> {
        let game_id = op
            .game_installation_id
            .ok_or_else(|| AppError::internal("SMAPI recovery lacks game ID", op.id.to_string()))?;
        let game = self
            .game_repo
            .get_game(&game_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game not found"))?;
        let observation = self.smapi_inspector.observe_smapi(&game.canonical_root)?;

        if !observation.is_present {
            return self.lifecycle.transition(
                &op.id,
                OperationState::Failed,
                Some("SMAPI_NOT_PRESENT"),
                Some("SMAPI was not present after the interrupted setup".to_string()),
            );
        }
        if let Some((error_code, message)) = smapi_recovery_failure(&observation) {
            return self.lifecycle.transition(
                &op.id,
                OperationState::Failed,
                Some(error_code),
                Some(message.to_string()),
            );
        }
        let Some(version) = observation.observed_version else {
            return Ok(());
        };
        let policy_id = self.smapi_release_policy_id(op)?;
        self.smapi_repo.save_smapi_installation(
            &manager_core::smapi::ManagedSmapiInstallation {
                game_installation_id: game_id,
                release_version: version,
                release_policy_id: policy_id,
                installed_at: observation.observed_at,
            },
        )?;
        self.lifecycle.transition(
            &op.id,
            OperationState::Succeeded,
            Some("RECOVERED_SMAPI_STATE"),
            Some("Recovered managed SMAPI state from filesystem evidence".to_string()),
        )
    }

    fn reconcile_v1_profile_operation(&self, op: &Operation) -> AppResult<()> {
        let profile_id = op.profile_id.ok_or_else(|| {
            AppError::internal("Recovery operation lacks profile ID", op.id.to_string())
        })?;
        let plan: serde_json::Value = serde_json::from_str(&op.plan_json)
            .map_err(|e| AppError::internal("Corrupted recovery plan", e.to_string()))?;
        let path = plan
            .get(if op.kind == OperationKind::ModInstall {
                "mod_folder_name"
            } else {
                "deployment_rel_path"
            })
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::internal("Missing recovery deployment path", op.id.to_string())
            })?;

        let recovery_result = if op.kind == OperationKind::ModInstall {
            if self.deployment.deployment_exists(&profile_id, path)? {
                self.deployment
                    .quarantine_deployment(&profile_id, &op.id, path)
                    .map(|_| ())
            } else {
                Ok(())
            }
        } else {
            self.deployment
                .restore_quarantined_deployment(&profile_id, &op.id, path)
        };

        if let Err(error) = recovery_result {
            // The operation keeps its recovery state, so a failed reconciliation
            // attempt reports recovery semantics too.
            return Err(error.into_recovery_required(op.id));
        }
        self.lifecycle.transition(
            &op.id,
            OperationState::Failed,
            Some(RECOVERED_TO_TERMINAL_STATE),
            Some(
                "Filesystem evidence was conservatively reconciled; retry with a fresh plan"
                    .to_string(),
            ),
        )
    }
}
impl OperationsService {
    // -----------------------------------------------------------------------
    // Plan-v2 deterministic recovery
    // -----------------------------------------------------------------------

    fn recover_v2_operation(&self, op: &Operation) -> AppResult<()> {
        match op.kind {
            OperationKind::ModInstall => self.recover_v2_install(op),
            OperationKind::ModRemove => self.recover_v2_removal(op),
            OperationKind::SmapiSetup => self.recover_v2_smapi(op),
            _ => Err(self.retain_v2_recovery(
                op,
                RECONCILIATION_REQUIRED,
                "Interrupted operation requires manual reconciliation; recovery files were preserved",
            )),
        }
    }

    /// Plan-v2 install recovery, decided from steps plus evidence.
    ///
    /// The proof is conjunctive: this operation's own deployment must be present
    /// in the authoritative database *and* the filesystem must be in the state
    /// that deployment implies. Neither half is allowed to short-circuit the
    /// other, and ambiguity is decided before either of them can claim success.
    fn recover_v2_install(&self, op: &Operation) -> AppResult<()> {
        let profile_id = op.profile_id.ok_or_else(|| {
            AppError::internal("Install recovery lacks profile ID", op.id.to_string())
        })?;
        let profile = self
            .profile_repo
            .get_profile(&profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        let plan = self.install_plan(op)?;
        let target = plan.mod_folder_name.clone();
        let steps = self.lifecycle.load_steps(&op.id)?;
        let expected_deployment = install_deployment_id(&steps);
        let deployment_id = expected_deployment.unwrap_or_default();

        let live = self.live_deployment(op, &profile_id, &target)?;
        let quarantined = self.quarantined_deployment(op, &profile_id, &target)?;

        // Ambiguity first. When both copies exist, nothing in the database can
        // tell us which one its record refers to, so no ownership claim is
        // allowed to turn this into success.
        if live && quarantined {
            return Err(self.retain_v2_recovery(
                op,
                "INSTALL_EVIDENCE_AMBIGUOUS",
                "Both a live and a quarantined copy of the deployment exist; manual inspection is required",
            ));
        }

        let owned = self.install_deployment_is_owned(
            &profile_id,
            &target,
            &plan.package_hash,
            expected_deployment,
        )?;

        if owned {
            // The database owns this operation's deployment. That is only half
            // the proof: the folder it describes must still be live, and no
            // quarantined copy may exist (already excluded above).
            if !live {
                return Err(self.retain_v2_recovery(
                    op,
                    "INSTALL_DEPLOYMENT_MISSING",
                    "The database owns this installation, but its deployment folder is not in the profile",
                ));
            }
            self.record_step_completed(
                op,
                &steps,
                INSTALL_STEP_PUBLISH_DEPLOYMENT,
                OperationStepKind::PublishDeployment,
                serde_json::json!({ "mod_folder_name": target, "recovered": true }),
            )?;
            self.record_step_completed(
                op,
                &steps,
                INSTALL_STEP_COMMIT_DATABASE,
                OperationStepKind::CommitInstallDatabase,
                serde_json::json!({ "mod_folder_name": target, "recovered": true }),
            )?;
            self.finish_install_cleanup(op, &profile_id)?;
            return self.lifecycle.transition(
                &op.id,
                OperationState::Succeeded,
                Some("RECOVERED_AFTER_INSTALL_COMMIT"),
                Some("The atomic install commit had already been applied".to_string()),
            );
        }

        if !live && quarantined {
            // The publication was moved into the recovery tree, so the live
            // profile is clean and the database never owned the folder.
            return self.lifecycle.transition(
                &op.id,
                OperationState::Failed,
                Some("INSTALL_ROLLED_BACK"),
                Some("The published folder was moved into the recovery tree and the database never owned it".to_string()),
            );
        }
        if live {
            // A folder this operation cannot prove is its own may not be adopted
            // by writing records that claim it is. Another deployment already
            // occupying the target path is exactly that situation.
            if self.foreign_deployment_occupies_path(
                &profile_id,
                &target,
                &plan.package_hash,
                expected_deployment,
            )? {
                return Err(self.retain_v2_recovery(
                    op,
                    "INSTALL_PATH_OCCUPIED",
                    "Another deployment already occupies this installation's target folder",
                ));
            }
            // The folder is live and the database does not own it: the
            // publication happened and the atomic commit did not.
            self.record_step_completed(
                op,
                &steps,
                INSTALL_STEP_PUBLISH_DEPLOYMENT,
                OperationStepKind::PublishDeployment,
                serde_json::json!({ "mod_folder_name": target, "recovered": true }),
            )?;
            if op.expected_profile_revision == Some(profile.revision) {
                return self.resume_v2_install_commit(op, &profile, deployment_id, &target);
            }
            // Adopting a folder for a plan the profile has moved past would
            // commit a stale preview, so the publication is compensated instead.
            return self.compensate_v2_install(op, &profile_id, &target);
        }

        // No live and no quarantined copy: publication provably never completed.
        let staged = match self
            .staging
            .staged_content_exists(&profile_id, &op.id, &target)
        {
            Ok(staged) => staged,
            Err(error) => {
                return Err(self
                    .retain_v2_recovery(
                        op,
                        "INSTALL_STAGING_EVIDENCE_UNREADABLE",
                        "The staged source of this installation could not be read",
                    )
                    .with_details(error.to_string()));
            }
        };
        if !staged {
            // Nothing live was created, so requiring a fresh plan is safe and
            // keeps the profile usable.
            return self.lifecycle.transition(
                &op.id,
                OperationState::Failed,
                Some("INSTALL_NOT_PUBLISHED"),
                Some(
                    "Nothing was published and the staged source is gone; retry with a fresh plan"
                        .to_string(),
                ),
            );
        }
        self.retry_v2_install_publication(op, &profile, &plan, deployment_id)
    }

    /// Whether a deployment that is not this operation's occupies the target.
    ///
    /// Adopting such a folder would record ownership of something this operation
    /// cannot prove it created.
    fn foreign_deployment_occupies_path(
        &self,
        profile_id: &ProfileId,
        target: &str,
        expected_artifact_hash: &str,
        expected_deployment_id: Option<DeploymentId>,
    ) -> AppResult<bool> {
        Ok(self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?
            .into_iter()
            .any(|deployment| {
                deployment.root_relative_path == target
                    && deployment.state != manager_core::deployment::DeploymentState::Quarantined
                    && !(deployment
                        .artifact_hash
                        .as_str()
                        .eq_ignore_ascii_case(expected_artifact_hash)
                        && expected_deployment_id.is_none_or(|id| id == deployment.id))
            }))
    }

    /// Whether the authoritative database owns *this operation's* deployment.
    ///
    /// A deployment at the same relative path is not proof on its own: another
    /// installation could have published a different artifact there. Ownership
    /// is therefore proven by the deployment identity recorded in this
    /// operation's publish step, the artifact hash frozen in its plan, and the
    /// profile components that the atomic commit created alongside it.
    fn install_deployment_is_owned(
        &self,
        profile_id: &ProfileId,
        target: &str,
        expected_artifact_hash: &str,
        expected_deployment_id: Option<DeploymentId>,
    ) -> AppResult<bool> {
        let deployments = self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?;
        let components = self.deployment_repo.list_profile_components(profile_id)?;

        Ok(deployments.into_iter().any(|deployment| {
            deployment.root_relative_path == target
                && deployment.state != manager_core::deployment::DeploymentState::Quarantined
                && deployment
                    .artifact_hash
                    .as_str()
                    .eq_ignore_ascii_case(expected_artifact_hash)
                && expected_deployment_id.is_none_or(|id| id == deployment.id)
                && components
                    .iter()
                    .any(|component| component.deployment_id == deployment.id)
        }))
    }

    /// Republishes a staged installation whose publication never completed.
    fn retry_v2_install_publication(
        &self,
        op: &Operation,
        profile: &Profile,
        plan: &manager_core::install::InstallPlan,
        deployment_id: DeploymentId,
    ) -> AppResult<()> {
        let profile_id = profile.id;
        let target = plan.mod_folder_name.clone();
        if op.state != OperationState::Committing {
            self.lifecycle
                .transition(&op.id, OperationState::Committing, None, None)?;
        }

        let staging_root = self.staging.create_staging_dir(&profile_id, &op.id)?;
        let staged_content = staging_root.join(&target);

        self.lifecycle.start_step(
            &op.id,
            INSTALL_STEP_PUBLISH_DEPLOYMENT,
            OperationStepKind::PublishDeployment,
            serde_json::json!({ "mod_folder_name": target, "deployment_id": deployment_id.to_string() }),
        )?;
        if let Err(error) = self.staging_verifier.verify_staged(plan, &staged_content) {
            self.lifecycle.fail_step(
                &op.id,
                INSTALL_STEP_PUBLISH_DEPLOYMENT,
                Some(error.to_string()),
            )?;
            return Err(self.retain_v2_recovery(
                op,
                "INSTALL_STAGED_SOURCE_INVALID",
                "The staged source no longer matches the prepared plan; prepare the installation again",
            )
            .with_details(error.to_string()));
        }
        if let Err(error) =
            self.deployment
                .publish_deployment(&profile_id, &staged_content, &target)
        {
            self.lifecycle.fail_step(
                &op.id,
                INSTALL_STEP_PUBLISH_DEPLOYMENT,
                Some(error.to_string()),
            )?;
            return Err(self.enter_recovery(
                op,
                "DEPLOYMENT_FAILED",
                "The deployment could not be retried during recovery",
                &error,
            ));
        }
        if !self.live_deployment(op, &profile_id, &target)? {
            return Err(self.retain_v2_recovery(
                op,
                "DEPLOYMENT_EVIDENCE_MISSING",
                "The retried publication could not be proven live",
            ));
        }
        self.lifecycle.complete_step(
            &op.id,
            INSTALL_STEP_PUBLISH_DEPLOYMENT,
            OperationStepKind::PublishDeployment,
            serde_json::json!({ "mod_folder_name": target, "deployment_id": deployment_id.to_string() }),
        )?;
        self.resume_v2_install_commit(op, profile, deployment_id, &target)
    }

    /// Applies the atomic install commit for a publication that is already live.
    fn resume_v2_install_commit(
        &self,
        op: &Operation,
        profile: &Profile,
        deployment_id: DeploymentId,
        target: &str,
    ) -> AppResult<()> {
        if op.state != OperationState::Committing {
            self.lifecycle
                .transition(&op.id, OperationState::Committing, None, None)?;
        }
        self.lifecycle.start_step(
            &op.id,
            INSTALL_STEP_COMMIT_DATABASE,
            OperationStepKind::CommitInstallDatabase,
            serde_json::json!({ "deployment_rel_path": target, "recovered": true }),
        )?;

        let commit = match self.build_install_commit(op, profile, deployment_id, target.to_string())
        {
            Ok(commit) => commit,
            Err(error) => {
                return Err(self
                    .retain_v2_recovery(
                        op,
                        "INSTALL_COMMIT_PAYLOAD_UNAVAILABLE",
                        "The database records needed to commit this installation could not be rebuilt",
                    )
                    .with_details(error.to_string()));
            }
        };
        if let Err(error) = self.mutation_store.commit_install(commit) {
            self.lifecycle.start_step(
                &op.id,
                INSTALL_STEP_QUARANTINE_PUBLISHED,
                OperationStepKind::QuarantinePublishedDeployment,
                serde_json::json!({ "deployment_rel_path": target }),
            )?;
            match self
                .deployment
                .quarantine_deployment(&profile.id, &op.id, target)
            {
                Ok(_) => {
                    self.lifecycle.complete_step(
                        &op.id,
                        INSTALL_STEP_QUARANTINE_PUBLISHED,
                        OperationStepKind::QuarantinePublishedDeployment,
                        serde_json::json!({ "deployment_rel_path": target }),
                    )?;
                    self.lifecycle.fail_step(
                        &op.id,
                        INSTALL_STEP_COMMIT_DATABASE,
                        Some(error.to_string()),
                    )?;
                    return self.lifecycle.transition(
                        &op.id,
                        OperationState::Failed,
                        Some("COMMIT_FAILED"),
                        Some(error.summary.clone()),
                    );
                }
                Err(rollback_error) => {
                    self.lifecycle.fail_step(
                        &op.id,
                        INSTALL_STEP_QUARANTINE_PUBLISHED,
                        Some(rollback_error.to_string()),
                    )?;
                    let cause = error.clone().with_details(format!(
                        "{}; the published folder could not be quarantined: {}",
                        error.summary, rollback_error
                    ));
                    return Err(self.enter_recovery(
                        op,
                        "COMMIT_FAILED",
                        "The installation could not be committed and its published folder could not be moved out of the profile",
                        &cause,
                    ));
                }
            }
        }
        self.lifecycle.complete_step(
            &op.id,
            INSTALL_STEP_COMMIT_DATABASE,
            OperationStepKind::CommitInstallDatabase,
            serde_json::json!({ "deployment_rel_path": target, "recovered": true }),
        )
    }

    /// Moves an uncommitted live publication into the recovery tree.
    fn compensate_v2_install(
        &self,
        op: &Operation,
        profile_id: &ProfileId,
        target: &str,
    ) -> AppResult<()> {
        let steps = self.lifecycle.load_steps(&op.id)?;
        self.lifecycle.start_step(
            &op.id,
            INSTALL_STEP_QUARANTINE_PUBLISHED,
            OperationStepKind::QuarantinePublishedDeployment,
            serde_json::json!({ "deployment_rel_path": target, "reason": "stale_plan" }),
        )?;
        match self
            .deployment
            .quarantine_deployment(profile_id, &op.id, target)
        {
            Ok(_) => {
                self.lifecycle.complete_step(
                    &op.id,
                    INSTALL_STEP_QUARANTINE_PUBLISHED,
                    OperationStepKind::QuarantinePublishedDeployment,
                    serde_json::json!({ "deployment_rel_path": target, "reason": "stale_plan" }),
                )?;
                // The commit step only exists once the commit was attempted.
                if steps
                    .iter()
                    .any(|step| step.step_index == INSTALL_STEP_COMMIT_DATABASE)
                {
                    self.lifecycle.fail_step(
                        &op.id,
                        INSTALL_STEP_COMMIT_DATABASE,
                        Some("PREVIEW_STALE".to_string()),
                    )?;
                }
                self.lifecycle.transition(
                    &op.id,
                    OperationState::Failed,
                    Some("INSTALL_COMPENSATED_STALE_PLAN"),
                    Some("The plan became stale after publication; the published folder was moved into the recovery tree".to_string()),
                )
            }
            Err(rollback_error) => Err(self.enter_recovery(
                op,
                "INSTALL_COMPENSATION_FAILED",
                "A stale publication could not be moved out of the profile",
                &rollback_error,
            )),
        }
    }
}
impl OperationsService {
    /// Plan-v2 removal recovery, decided from steps plus evidence.
    ///
    /// The proof is conjunctive in both directions: the exact planned deployment
    /// must be marked removed in the authoritative database *and* the folder it
    /// names must be absent from the live profile. Database state alone can no
    /// longer end the operation while the folder is still live, and a live
    /// folder alone can no longer be treated as a completed removal.
    fn recover_v2_removal(&self, op: &Operation) -> AppResult<()> {
        let profile_id = op.profile_id.ok_or_else(|| {
            AppError::internal("Removal recovery lacks profile ID", op.id.to_string())
        })?;
        let profile = self
            .profile_repo
            .get_profile(&profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        // The same strict decoder execution uses. Recovery cannot simply refuse
        // the operation the way preflight does - it may already have crossed a
        // live boundary - so an unreadable plan is a recovery situation here.
        let RemovalPlan {
            deployment_id,
            deployment_rel_path: target,
            removed_profile_component_ids,
        } = match RemovalPlan::parse(op) {
            Ok(plan) => plan,
            Err(detail) => {
                return Err(self
                    .retain_v2_recovery(
                        op,
                        crate::error::REMOVAL_PLAN_INVALID,
                        "The removal plan does not describe the components it removes",
                    )
                    .with_details(detail));
            }
        };

        let steps = self.lifecycle.load_steps(&op.id)?;
        let live = self.live_deployment(op, &profile_id, &target)?;
        let quarantined = self.quarantined_deployment(op, &profile_id, &target)?;

        // The exact planned deployment, not merely "some deployment at this
        // path": a different deployment there must not be mistaken for proof.
        let removed_in_database = match self.deployment_repo.get_deployment(&deployment_id)? {
            Some(deployment) => {
                if deployment.root_relative_path != target || deployment.profile_id != profile_id {
                    return Err(self.retain_v2_recovery(
                        op,
                        "REMOVAL_PLAN_MISMATCH",
                        "The planned deployment does not match the removal plan",
                    ));
                }
                deployment.state == manager_core::deployment::DeploymentState::Quarantined
            }
            None => true,
        };

        if live && quarantined {
            return Err(self.retain_v2_recovery(
                op,
                "REMOVAL_EVIDENCE_AMBIGUOUS",
                "Both a live and a quarantined copy of the deployment exist; manual inspection is required",
            ));
        }

        if removed_in_database {
            if live {
                // The database says the removal committed while the folder it
                // removed is still in the profile: nothing here is provable.
                return Err(self.retain_v2_recovery(
                    op,
                    "REMOVAL_EVIDENCE_INCONSISTENT",
                    "The removal is committed in the database while its deployment is still live",
                ));
            }
            self.record_step_completed(
                op,
                &steps,
                REMOVAL_STEP_QUARANTINE_DEPLOYMENT,
                OperationStepKind::QuarantineDeployment,
                serde_json::json!({ "deployment_rel_path": target, "recovered": true }),
            )?;
            self.record_step_completed(
                op,
                &steps,
                REMOVAL_STEP_COMMIT_DATABASE,
                OperationStepKind::CommitRemovalDatabase,
                serde_json::json!({ "deployment_rel_path": target, "recovered": true }),
            )?;
            return self.lifecycle.transition(
                &op.id,
                OperationState::Succeeded,
                Some("RECOVERED_AFTER_REMOVAL_COMMIT"),
                Some("The atomic removal commit had already been applied".to_string()),
            );
        }

        if !live && !quarantined {
            // The database still owns a deployment whose folder is nowhere:
            // guessing would be worse than asking.
            return Err(self.retain_v2_recovery(
                op,
                "REMOVAL_EVIDENCE_MISSING",
                "The deployment is missing from both the profile and the recovery tree while the database still owns it",
            ));
        }

        if live {
            // The filesystem removal never became durable, so retrying the
            // quarantine is safe.
            self.lifecycle.start_step(
                &op.id,
                REMOVAL_STEP_QUARANTINE_DEPLOYMENT,
                OperationStepKind::QuarantineDeployment,
                serde_json::json!({ "deployment_rel_path": target, "recovered": true }),
            )?;
            if let Err(error) = self
                .deployment
                .quarantine_deployment(&profile_id, &op.id, &target)
            {
                self.lifecycle.fail_step(
                    &op.id,
                    REMOVAL_STEP_QUARANTINE_DEPLOYMENT,
                    Some(error.to_string()),
                )?;
                return Err(self.enter_recovery(
                    op,
                    "QUARANTINE_FAILED",
                    "The deployment could not be moved into the recovery tree",
                    &error,
                ));
            }
        }

        let live_now = self.live_deployment(op, &profile_id, &target)?;
        let quarantined_now = self.quarantined_deployment(op, &profile_id, &target)?;
        if live_now || !quarantined_now {
            return Err(self.retain_v2_recovery(
                op,
                "REMOVAL_EVIDENCE_INCONSISTENT",
                "The deployment is neither fully removed nor fully recovered",
            ));
        }
        self.record_step_completed(
            op,
            &steps,
            REMOVAL_STEP_QUARANTINE_DEPLOYMENT,
            OperationStepKind::QuarantineDeployment,
            serde_json::json!({ "deployment_rel_path": target, "recovered": true }),
        )?;

        // The filesystem removal is durable and the database still owns the
        // deployment, so the atomic removal commit is what is left to apply.
        if op.state != OperationState::Committing {
            self.lifecycle
                .transition(&op.id, OperationState::Committing, None, None)?;
        }
        self.lifecycle.start_step(
            &op.id,
            REMOVAL_STEP_COMMIT_DATABASE,
            OperationStepKind::CommitRemovalDatabase,
            serde_json::json!({ "deployment_rel_path": target, "recovered": true }),
        )?;

        if let Err(error) = self.mutation_store.commit_removal(RemovalCommit {
            operation_id: op.id,
            profile_id,
            expected_profile_revision: op.expected_profile_revision.unwrap_or(profile.revision),
            deployment_id,
            removed_profile_component_ids,
            effects: Vec::new(),
            commit_step: self.removal_commit_step(op, &target),
        }) {
            self.lifecycle.start_step(
                &op.id,
                REMOVAL_STEP_RESTORE_QUARANTINED,
                OperationStepKind::RestoreQuarantinedDeployment,
                serde_json::json!({ "deployment_rel_path": target }),
            )?;
            match self
                .deployment
                .restore_quarantined_deployment(&profile_id, &op.id, &target)
            {
                Ok(()) => {
                    self.lifecycle.complete_step(
                        &op.id,
                        REMOVAL_STEP_RESTORE_QUARANTINED,
                        OperationStepKind::RestoreQuarantinedDeployment,
                        serde_json::json!({ "deployment_rel_path": target }),
                    )?;
                    self.lifecycle.fail_step(
                        &op.id,
                        REMOVAL_STEP_COMMIT_DATABASE,
                        Some(error.to_string()),
                    )?;
                    return self.lifecycle.transition(
                        &op.id,
                        OperationState::Failed,
                        Some("COMMIT_FAILED"),
                        Some(error.summary.clone()),
                    );
                }
                Err(restore_error) => {
                    self.lifecycle.fail_step(
                        &op.id,
                        REMOVAL_STEP_RESTORE_QUARANTINED,
                        Some(restore_error.to_string()),
                    )?;
                    let cause = error.clone().with_details(format!(
                        "{}; the deployment could not be restored: {}",
                        error.summary, restore_error
                    ));
                    return Err(self.enter_recovery(
                        op,
                        "COMMIT_FAILED",
                        "The removal could not be committed and its deployment could not be restored",
                        &cause,
                    ));
                }
            }
        }
        Ok(())
    }

    /// Plan-v2 SMAPI recovery.
    ///
    /// The installer mutates the game directory before the managed state is
    /// written, so the two halves are reconciled against observed evidence.
    fn recover_v2_smapi(&self, op: &Operation) -> AppResult<()> {
        let game_id = op
            .game_installation_id
            .ok_or_else(|| AppError::internal("SMAPI recovery lacks game ID", op.id.to_string()))?;
        let game = self
            .game_repo
            .get_game(&game_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game not found"))?;
        let observation = self.smapi_inspector.observe_smapi(&game.canonical_root)?;
        let steps = self.lifecycle.load_steps(&op.id)?;

        if !observation.is_present {
            self.record_step_failed(
                op,
                &steps,
                SMAPI_STEP_INSTALL_FILES,
                OperationStepKind::InstallSmapiFiles,
                "SMAPI_INSTALL_FAILED",
            )?;
            return self.lifecycle.transition(
                &op.id,
                OperationState::Failed,
                Some("SMAPI_NOT_PRESENT"),
                Some("SMAPI was not present after the interrupted setup".to_string()),
            );
        }

        if let Some((error_code, message)) = smapi_recovery_failure(&observation) {
            return self.lifecycle.transition(
                &op.id,
                OperationState::Failed,
                Some(error_code),
                Some(message.to_string()),
            );
        }

        let Some(version) = observation.observed_version else {
            return Ok(());
        };
        self.record_step_completed(
            op,
            &steps,
            SMAPI_STEP_INSTALL_FILES,
            OperationStepKind::InstallSmapiFiles,
            serde_json::json!({ "observed_version": version, "recovered": true }),
        )?;

        self.lifecycle.start_step(
            &op.id,
            SMAPI_STEP_PERSIST_STATE,
            OperationStepKind::PersistSmapiState,
            serde_json::json!({ "observed_version": version.clone(), "recovered": true }),
        )?;
        let policy_id = self.smapi_release_policy_id(op)?;
        self.smapi_repo.save_smapi_installation(
            &manager_core::smapi::ManagedSmapiInstallation {
                game_installation_id: game_id,
                release_version: version.clone(),
                release_policy_id: policy_id,
                installed_at: observation.observed_at,
            },
        )?;
        self.lifecycle.complete_step(
            &op.id,
            SMAPI_STEP_PERSIST_STATE,
            OperationStepKind::PersistSmapiState,
            serde_json::json!({ "observed_version": version, "recovered": true }),
        )?;

        self.lifecycle.transition(
            &op.id,
            OperationState::Succeeded,
            Some("RECOVERED_SMAPI_STATE"),
            Some("Recovered managed SMAPI state from filesystem evidence".to_string()),
        )
    }

    /// The release policy recorded in a SMAPI plan, if the plan carries one.
    fn smapi_release_policy_id(&self, op: &Operation) -> AppResult<String> {
        let plan: serde_json::Value = serde_json::from_str(&op.plan_json)
            .map_err(|e| AppError::internal("Corrupted SMAPI recovery plan", e.to_string()))?;
        Ok(plan
            .get("release_policy_id")
            .and_then(|v| v.as_str())
            .unwrap_or("recovered")
            .to_string())
    }

    // -----------------------------------------------------------------------
    // Evidence helpers
    // -----------------------------------------------------------------------

    /// Whether the managed folder is live in the profile.
    ///
    /// An unreadable query is not absence: it leaves the operation in
    /// `RecoveryRequired` instead of letting recovery act on a guess.
    fn live_deployment(
        &self,
        op: &Operation,
        profile_id: &ProfileId,
        target: &str,
    ) -> AppResult<bool> {
        self.deployment
            .deployment_exists(profile_id, target)
            .map_err(|error| {
                self.retain_v2_recovery(
                    op,
                    "RECOVERY_EVIDENCE_UNREADABLE",
                    "The live deployment evidence could not be read",
                )
                .with_details(error.to_string())
            })
    }

    /// Whether the quarantined copy exists in the trusted recovery tree.
    fn quarantined_deployment(
        &self,
        op: &Operation,
        profile_id: &ProfileId,
        target: &str,
    ) -> AppResult<bool> {
        self.deployment
            .recovery_deployment_exists(profile_id, &op.id, target)
            .map_err(|error| {
                self.retain_v2_recovery(
                    op,
                    "RECOVERY_EVIDENCE_UNREADABLE",
                    "The recovery-tree evidence could not be read",
                )
                .with_details(error.to_string())
            })
    }

    /// Persists a step as completed unless it already is.
    fn record_step_completed(
        &self,
        op: &Operation,
        steps: &[manager_core::operation::OperationStep],
        index: u32,
        kind: OperationStepKind,
        payload: serde_json::Value,
    ) -> AppResult<()> {
        if step_state_of(steps, kind)
            == Some(manager_core::operation::OperationStepState::Completed)
        {
            return Ok(());
        }
        self.lifecycle.complete_step(&op.id, index, kind, payload)
    }

    /// Persists a step as failed unless it already is.
    fn record_step_failed(
        &self,
        op: &Operation,
        steps: &[manager_core::operation::OperationStep],
        index: u32,
        kind: OperationStepKind,
        reason: &str,
    ) -> AppResult<()> {
        if step_state_of(steps, kind) == Some(manager_core::operation::OperationStepState::Failed) {
            return Ok(());
        }
        if step_state_of(steps, kind).is_none() {
            self.lifecycle.start_step(
                &op.id,
                index,
                kind,
                serde_json::json!({ "reason": reason }),
            )?;
        }
        self.lifecycle
            .fail_step(&op.id, index, Some(format!("{{\"code\":\"{reason}\"}}")))
    }

    /// Records that an operation needs manual reconciliation.
    pub(crate) fn retain_v2_recovery(&self, op: &Operation, code: &str, message: &str) -> AppError {
        let evidence = serde_json::json!({ "code": code, "message": message }).to_string();
        let diagnosis = AppError::recovery_required(code, message, op.id);
        match self.lifecycle.transition(
            &op.id,
            OperationState::RecoveryRequired,
            Some(code),
            Some(evidence.clone()),
        ) {
            Ok(()) => {
                let _ = self.lifecycle.record_error(&op.id, code, Some(evidence));
                diagnosis
            }
            Err(persist_error) => crate::services::operations::recovery_state_unknown(
                op.id,
                &diagnosis,
                &persist_error,
            ),
        }
    }
}

/// The persisted state of a named step.
fn step_state_of(
    steps: &[manager_core::operation::OperationStep],
    kind: OperationStepKind,
) -> Option<manager_core::operation::OperationStepState> {
    steps
        .iter()
        .find(|step| OperationStepKind::parse(&step.step_kind) == Some(kind))
        .map(|step| step.state)
}

/// The deployment identity recorded by the publish step, if it reached one.
fn install_deployment_id(steps: &[manager_core::operation::OperationStep]) -> Option<DeploymentId> {
    steps
        .iter()
        .find(|step| {
            OperationStepKind::parse(&step.step_kind) == Some(OperationStepKind::PublishDeployment)
        })
        .and_then(|step| serde_json::from_str::<serde_json::Value>(&step.payload_json).ok())
        .and_then(|payload| {
            payload
                .get("deployment_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .and_then(|value| DeploymentId::from_str(&value).ok())
}
