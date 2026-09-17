use crate::api::dto::OperationDto;
use crate::error::{AppError, AppResult};
use crate::ports::deployment::{DeploymentPort, StagedContentVerifierPort, StagingPort};
use crate::ports::launcher::GameLauncherPort;
use crate::ports::repositories::{
    AtomicMutationStore, CommitStep, DeploymentRepository, GameInstallationRepository,
    InstallCommit, OperationRepository, PackageCatalogRepository, ProfileRepository, RemovalCommit,
    SmapiRepository,
};
use crate::ports::runtime::SmapiInspectorPort;
use crate::services::operation_lifecycle::OperationLifecycle;
use crate::services::resources::{ensure_resources_available, ResourceClaim, ResourceCoordinator};
use chrono::Utc;
use manager_core::deployment::{InstalledReason, ProfileComponent, ProfileDeployment};
use manager_core::ids::{ArtifactHash, DeploymentId, OperationId, ProfileComponentId, ProfileId};
use manager_core::install::InstallPlan;
use manager_core::operation::{
    Operation, OperationEffect, OperationKind, OperationState, OperationStepKind, ResourceKind,
    INSTALL_STEP_CLEANUP_STAGING, INSTALL_STEP_COMMIT_DATABASE, INSTALL_STEP_PUBLISH_DEPLOYMENT,
    INSTALL_STEP_QUARANTINE_PUBLISHED, REMOVAL_STEP_COMMIT_DATABASE,
    REMOVAL_STEP_QUARANTINE_DEPLOYMENT, REMOVAL_STEP_RESTORE_QUARANTINED,
};
use manager_core::package::PackageComponent;
use manager_core::ports::InstanceLock;
use manager_core::profile::Profile;
use std::str::FromStr;
use std::sync::Arc;

/// The persisted removal plan, decoded strictly.
///
/// Execution and recovery share this decoder, so a plan this build cannot fully
/// understand fails identically on both paths instead of quietly removing fewer
/// components than the plan names. Every field is required.
pub(crate) struct RemovalPlan {
    pub deployment_id: DeploymentId,
    pub deployment_rel_path: String,
    pub removed_profile_component_ids: Vec<ProfileComponentId>,
}

impl RemovalPlan {
    /// Decodes a persisted removal plan, or explains what could not be read.
    pub(crate) fn parse(op: &Operation) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(&op.plan_json)
            .map_err(|e| format!("the removal plan is not valid JSON: {}", e))?;

        let raw_deployment_id = required_string(&value, "deployment_id")?;
        let deployment_id = DeploymentId::from_str(&raw_deployment_id)
            .map_err(|e| format!("deployment_id '{}' is invalid: {}", raw_deployment_id, e))?;

        let deployment_rel_path = required_string(&value, "deployment_rel_path")?;
        manager_core::install::validate_relative_path(&deployment_rel_path)
            .map_err(|e| format!("deployment_rel_path is not a valid managed path: {}", e))?;

        let values = value
            .get("removed_profile_component_ids")
            .and_then(|value| value.as_array())
            .ok_or_else(|| "the plan does not list removed_profile_component_ids".to_string())?;
        if values.is_empty() {
            return Err("the plan lists no components to remove".to_string());
        }

        let mut removed_profile_component_ids = Vec::with_capacity(values.len());
        for item in values {
            let raw = item
                .as_str()
                .ok_or_else(|| "a removed component id is not a string".to_string())?;
            removed_profile_component_ids.push(
                ProfileComponentId::from_str(raw)
                    .map_err(|e| format!("removed component id '{}' is invalid: {}", raw, e))?,
            );
        }

        Ok(Self {
            deployment_id,
            deployment_rel_path,
            removed_profile_component_ids,
        })
    }
}

fn required_string(value: &serde_json::Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("the plan does not contain a usable '{}'", key))
}

pub(crate) const EXECUTION_INTERRUPTED: &str = "EXECUTION_INTERRUPTED";

/// The execution engine for durable install/remove operations.
///
/// Every live side effect is bracketed by a persisted step: `Running` before the
/// effect is attempted, `Completed` only once evidence confirms it happened.
/// Restart recovery reads those steps back together with concrete filesystem and
/// database evidence, which is what makes it deterministic instead of guessing
/// from the coarse operation state.
pub struct OperationsService {
    pub(crate) lifecycle: OperationLifecycle,
    pub(crate) resources: Arc<ResourceCoordinator>,
    pub(crate) operation_repo: Arc<dyn OperationRepository>,
    pub(crate) profile_repo: Arc<dyn ProfileRepository>,
    pub(crate) deployment_repo: Arc<dyn DeploymentRepository>,
    pub(crate) package_repo: Arc<dyn PackageCatalogRepository>,
    pub(crate) mutation_store: Arc<dyn AtomicMutationStore>,
    pub(crate) deployment: Arc<dyn DeploymentPort>,
    pub(crate) staging: Arc<dyn StagingPort>,
    pub(crate) staging_verifier: Arc<dyn StagedContentVerifierPort>,
    pub(crate) launcher: Arc<dyn GameLauncherPort>,
    pub(crate) instance_lock: Arc<dyn InstanceLock>,
    pub(crate) game_repo: Arc<dyn GameInstallationRepository>,
    pub(crate) smapi_inspector: Arc<dyn SmapiInspectorPort>,
    pub(crate) smapi_repo: Arc<dyn SmapiRepository>,
}

impl OperationsService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resources: Arc<ResourceCoordinator>,
        operation_repo: Arc<dyn OperationRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        mutation_store: Arc<dyn AtomicMutationStore>,
        deployment: Arc<dyn DeploymentPort>,
        staging: Arc<dyn StagingPort>,
        staging_verifier: Arc<dyn StagedContentVerifierPort>,
        launcher: Arc<dyn GameLauncherPort>,
        instance_lock: Arc<dyn InstanceLock>,
        game_repo: Arc<dyn GameInstallationRepository>,
        smapi_inspector: Arc<dyn SmapiInspectorPort>,
        smapi_repo: Arc<dyn SmapiRepository>,
    ) -> Self {
        Self {
            lifecycle: OperationLifecycle::new(operation_repo.clone()),
            resources,
            operation_repo,
            profile_repo,
            deployment_repo,
            package_repo,
            mutation_store,
            deployment,
            staging,
            staging_verifier,
            launcher,
            instance_lock,
            game_repo,
            smapi_inspector,
            smapi_repo,
        }
    }

    pub fn get_operation(&self, id: &OperationId) -> AppResult<Option<OperationDto>> {
        let op = self.operation_repo.get_operation(id)?;
        Ok(op.map(|o| Self::op_to_dto(&o)))
    }

    pub fn list_operations(&self, profile_id: Option<&ProfileId>) -> AppResult<Vec<OperationDto>> {
        let ops = if let Some(pid) = profile_id {
            self.operation_repo.list_operations_for_profile(pid)?
        } else {
            self.operation_repo.list_recent_operations(100)?
        };
        Ok(ops.into_iter().map(|o| Self::op_to_dto(&o)).collect())
    }

    /// Cancels a preview that never entered its mutation lifecycle.
    pub fn cancel_operation(&self, id: &OperationId) -> AppResult<()> {
        let op = self
            .operation_repo
            .get_operation(id)?
            .ok_or_else(|| AppError::validation("OPERATION_NOT_FOUND", "Operation not found"))?;

        if op.state != OperationState::Draft && op.state != OperationState::Prepared {
            return Err(AppError::operation_not_cancellable(id));
        }

        if let Some(pid) = op.profile_id {
            // Nothing live was ever created for a preview, so cleaning the
            // staging tree is housekeeping rather than an authoritative effect.
            let _ = self.staging.clean_staging_dir(&pid, id);
        }
        self.lifecycle.transition(
            id,
            OperationState::Cancelled,
            None,
            Some("Cancelled by user".to_string()),
        )
    }

    pub fn commit_operation(&self, id: &OperationId) -> AppResult<OperationDto> {
        let mut op = self
            .operation_repo
            .get_operation(id)?
            .ok_or_else(|| AppError::validation("OPERATION_NOT_FOUND", "Operation not found"))?;

        if op.state != OperationState::Draft && op.state != OperationState::Prepared {
            return Err(AppError::validation(
                "INVALID_OPERATION_STATE",
                format!("Operation {} is in state {:?}, cannot commit", id, op.state),
            ));
        }

        // Stale-plan protection: revalidate the profile revision the preview was
        // validated against before anything live happens.
        let profile_id = op
            .profile_id
            .ok_or_else(|| AppError::internal("Operation lacks profile ID", id.to_string()))?;

        // Durable ownership first: an unresolved operation from an earlier run
        // still owns its declared resources, even though no in-process lease
        // survived the restart.
        let mut claims = self.operation_claims(&op)?;
        if claims.is_empty() {
            claims.push(ResourceClaim::write(
                ResourceKind::Profile,
                profile_id.to_string(),
            ));
        }
        ensure_resources_available(&*self.operation_repo, &claims, Some(id))?;
        // Then in-process ownership: what is executing right now.
        let _resource_lease = self.resources.try_acquire(&claims)?;

        let _mutation_guard = self
            .instance_lock
            .acquire_guard()
            .map_err(AppError::instance_locked)?;
        if self.launcher.is_game_running(None) {
            return Err(AppError::game_running(
                "Stop Stardew Valley before changing managed files",
            ));
        }

        let profile = self
            .profile_repo
            .get_profile(&profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        if profile.state != manager_core::profile::ProfileState::Active {
            return Err(AppError::validation(
                "PROFILE_NOT_ACTIVE",
                "Only active profiles can be mutated",
            ));
        }

        if let Some(expected_rev) = op.expected_profile_revision {
            if profile.revision != expected_rev {
                return Err(AppError::preview_stale(expected_rev, profile.revision));
            }
        }

        // Every persisted plan has to be fully readable before anything live
        // happens. A removal whose plan cannot be decoded could otherwise run as
        // a partial removal, so this check deliberately happens before the
        // operation enters its mutation lifecycle.
        match op.kind {
            OperationKind::ModInstall => {
                let plan = self.install_plan(&op)?;
                if !plan.dependency_report.is_installable {
                    return Err(AppError::validation(
                        "INSTALL_BLOCKED",
                        "Resolve the installation blockers before installing",
                    ));
                }
            }
            OperationKind::ModRemove => {
                RemovalPlan::parse(&op).map_err(AppError::removal_plan_invalid)?;
            }
            _ => {}
        }

        // Persist the validated preflight boundary before entering mutation.
        if op.state == OperationState::Draft {
            self.lifecycle
                .transition(id, OperationState::Prepared, None, None)?;
            op.state = OperationState::Prepared;
        }

        // Transition through the explicit committing boundary before any
        // filesystem/database mutation begins. Atomic mutation commits finish
        // the Committing -> Succeeded transition in the same DB transaction.
        self.lifecycle
            .transition(id, OperationState::Running, None, None)?;
        self.lifecycle
            .transition(id, OperationState::Committing, None, None)?;
        op.state = OperationState::Committing;

        let result = match op.kind {
            OperationKind::ModInstall => self.execute_install_commit(&op, &profile),
            OperationKind::ModRemove => self.execute_removal_commit(&op, &profile),
            _ => Err(AppError::validation(
                "UNSUPPORTED_OPERATION_KIND",
                format!("Cannot commit operation kind {:?}", op.kind),
            )),
        };

        let Err(error) = result else {
            return result;
        };

        // The failure may already have left the operation in a terminal state and
        // recorded its own evidence; only promote a genuinely interrupted
        // operation to RecoveryRequired.
        let still_mutating = match self.operation_repo.get_operation(id) {
            Ok(Some(current)) => matches!(
                current.state,
                OperationState::Running
                    | OperationState::Committing
                    | OperationState::RollingBack
                    | OperationState::Cancelling
            ),
            Ok(None) => false,
            // Without knowing the persisted state, claiming that RecoveryRequired
            // was recorded would be a guess.
            Err(read_error) => {
                return Err(recovery_state_unknown(*id, &error, &read_error));
            }
        };
        if !still_mutating {
            return Err(error);
        }

        match self.lifecycle.transition(
            id,
            OperationState::RecoveryRequired,
            Some(EXECUTION_INTERRUPTED),
            Some(
                "Operation failed after entering the mutation phase; reconciliation is required"
                    .to_string(),
            ),
        ) {
            Ok(()) => Err(error.into_recovery_required(*id)),
            Err(persist_error) => Err(recovery_state_unknown(*id, &error, &persist_error)),
        }
    }
    /// The parsed install plan of an operation.
    pub(crate) fn install_plan(&self, op: &Operation) -> AppResult<InstallPlan> {
        serde_json::from_str(&op.plan_json)
            .map_err(|e| AppError::internal("Corrupted install plan JSON", e.to_string()))
    }

    /// The durable resource claims of one operation.
    pub(crate) fn operation_claims(&self, op: &Operation) -> AppResult<Vec<ResourceClaim>> {
        Ok(self
            .operation_repo
            .list_operation_resources(&op.id)?
            .into_iter()
            .map(|resource| ResourceClaim {
                kind: resource.resource_kind,
                resource_id: resource.resource_id,
                mode: resource.access_mode,
            })
            .collect())
    }

    /// Builds the atomic install payload from the frozen plan.
    ///
    /// Execution and restart recovery use the same builder so a recovered commit
    /// produces exactly the records the interrupted run intended.
    pub(crate) fn build_install_commit(
        &self,
        op: &Operation,
        profile: &Profile,
        deployment_id: DeploymentId,
        target_relative_path: String,
    ) -> AppResult<InstallCommit> {
        let plan = self.install_plan(op)?;
        let artifact_hash = ArtifactHash::parse(&plan.package_hash)
            .map_err(|e| AppError::validation("INVALID_ARTIFACT_HASH", e.to_string()))?;
        let artifact = self
            .package_repo
            .get_artifact(&artifact_hash)?
            .ok_or_else(|| {
                AppError::internal("Artifact record missing", artifact_hash.to_string())
            })?;
        let acquisition = self
            .package_repo
            .get_acquisitions_for_artifact(&artifact_hash)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                AppError::internal("Acquisition record missing", artifact_hash.to_string())
            })?;

        // The commit step records the same relative path as the deployment, so
        // capture it before the deployment takes ownership of the string.
        let target_for_commit_step = target_relative_path.clone();
        let deployment_target = target_relative_path;

        let mut package_components = Vec::new();
        let mut profile_components = Vec::new();
        let mut effects = Vec::new();

        if plan.component_manifests.is_empty() {
            // A normal mod is the component at the package root. Its
            // deployment folder is materialised separately and must not be
            // part of the immutable component identity.
            let relative_component_root = String::new();
            let comp_id = PackageComponent::canonical_id(
                &artifact_hash,
                &relative_component_root,
                &plan.manifest.unique_id,
            );
            package_components.push(PackageComponent {
                id: comp_id,
                artifact_hash: artifact_hash.clone(),
                unique_id: plan.manifest.unique_id.clone(),
                name: plan.manifest.name.clone(),
                author: plan.manifest.author.clone(),
                version: plan.manifest.version.clone(),
                description: plan.manifest.description.clone(),
                relative_component_root,
                raw_manifest: plan.raw_manifest.clone(),
                manifest: plan.manifest.clone(),
            });

            let prof_comp_id = ProfileComponentId::new();
            profile_components.push(ProfileComponent {
                id: prof_comp_id,
                profile_id: profile.id,
                deployment_id,
                package_component_id: comp_id,
                enabled: true,
                installed_reason: InstalledReason::Direct,
            });
            effects.push(OperationEffect {
                id: uuid::Uuid::new_v4().to_string(),
                operation_id: op.id,
                profile_id: Some(profile.id),
                entity_type: "profile_component".to_string(),
                entity_id: prof_comp_id.to_string(),
                change_kind: "ProfileComponentAdded".to_string(),
                before_json: None,
                after_json: serde_json::to_string(&plan.manifest).ok(),
                occurred_at: Utc::now(),
            });
        } else {
            for comp in &plan.component_manifests {
                let comp_id = PackageComponent::canonical_id(
                    &artifact_hash,
                    &comp.relative_subfolder,
                    &comp.manifest.unique_id,
                );
                package_components.push(PackageComponent {
                    id: comp_id,
                    artifact_hash: artifact_hash.clone(),
                    unique_id: comp.manifest.unique_id.clone(),
                    name: comp.manifest.name.clone(),
                    author: comp.manifest.author.clone(),
                    version: comp.manifest.version.clone(),
                    description: comp.manifest.description.clone(),
                    relative_component_root: comp.relative_subfolder.clone(),
                    raw_manifest: comp.raw_manifest.clone(),
                    manifest: comp.manifest.clone(),
                });

                let prof_comp_id = ProfileComponentId::new();
                profile_components.push(ProfileComponent {
                    id: prof_comp_id,
                    profile_id: profile.id,
                    deployment_id,
                    package_component_id: comp_id,
                    enabled: true,
                    installed_reason: InstalledReason::BundleCompanion,
                });
                effects.push(OperationEffect {
                    id: uuid::Uuid::new_v4().to_string(),
                    operation_id: op.id,
                    profile_id: Some(profile.id),
                    entity_type: "profile_component".to_string(),
                    entity_id: prof_comp_id.to_string(),
                    change_kind: "ProfileComponentAdded".to_string(),
                    before_json: None,
                    after_json: serde_json::to_string(&comp.manifest).ok(),
                    occurred_at: Utc::now(),
                });
            }
        }

        Ok(InstallCommit {
            operation_id: op.id,
            profile_id: profile.id,
            expected_profile_revision: op.expected_profile_revision.unwrap_or(profile.revision),
            artifact,
            acquisition,
            package_components,
            deployment: ProfileDeployment {
                id: deployment_id,
                profile_id: profile.id,
                artifact_hash,
                root_relative_path: deployment_target,
                installed_at: Utc::now(),
                state: manager_core::deployment::DeploymentState::Present,
            },
            profile_components,
            effects,
            commit_step: self.install_commit_step(op, &target_for_commit_step),
        })
    }

    /// The v2 install execution lifecycle.
    ///
    /// `publish_deployment` -> `commit_install_database` -> `cleanup_staging`,
    /// with `quarantine_published_deployment` persisted only when the commit
    /// fails after publication actually happened.
    fn execute_install_commit(&self, op: &Operation, profile: &Profile) -> AppResult<OperationDto> {
        let plan = self.install_plan(op)?;
        let profile_id = profile.id;
        let target_relative_path = plan.mod_folder_name.clone();
        let deployment_id = DeploymentId::new();

        let staging_root = self.staging.create_staging_dir(&profile_id, &op.id)?;
        let staged_content_dir = staging_root.join(&target_relative_path);

        // Step 4: make the deployment live.
        self.lifecycle.start_step(
            &op.id,
            INSTALL_STEP_PUBLISH_DEPLOYMENT,
            OperationStepKind::PublishDeployment,
            serde_json::json!({
                "profile_id": profile_id.to_string(),
                "mod_folder_name": target_relative_path,
                "deployment_id": deployment_id.to_string(),
            }),
        )?;

        if let Err(error) = self
            .staging_verifier
            .verify_staged(&plan, &staged_content_dir)
        {
            self.lifecycle.fail_step(
                &op.id,
                INSTALL_STEP_PUBLISH_DEPLOYMENT,
                Some(error.to_string()),
            )?;
            self.lifecycle.transition(
                &op.id,
                OperationState::Failed,
                Some("VERIFICATION_FAILED"),
                Some(error.to_string()),
            )?;
            return Err(error);
        }

        if let Err(error) = self.deployment.publish_deployment(
            &profile_id,
            &staged_content_dir,
            &target_relative_path,
        ) {
            // The adapter proves nothing was moved when the destination was
            // already occupied; anything else has to be decided by evidence, and
            // an unreadable evidence query is not evidence of absence.
            let published = if error.code == "DEPLOYMENT_DESTINATION_EXISTS" {
                false
            } else {
                match self
                    .deployment
                    .deployment_exists(&profile_id, &target_relative_path)
                {
                    Ok(published) => published,
                    Err(evidence_error) => {
                        return Err(self.enter_recovery(
                            op,
                            "DEPLOYMENT_EVIDENCE_UNREADABLE",
                            "The published folder could not be verified after a failed publication",
                            &error.clone().with_details(format!(
                                "{}; evidence query failed: {}",
                                error.summary, evidence_error
                            )),
                        ));
                    }
                }
            };

            self.lifecycle.fail_step(
                &op.id,
                INSTALL_STEP_PUBLISH_DEPLOYMENT,
                Some(error.to_string()),
            )?;
            if !published {
                self.lifecycle.transition(
                    &op.id,
                    OperationState::Failed,
                    Some("DEPLOYMENT_FAILED"),
                    Some(error.summary.clone()),
                )?;
                return Err(error);
            }
            return Err(self.enter_recovery(
                op,
                "DEPLOYMENT_FAILED",
                "The deployment may be live in the profile even though publication failed",
                &error,
            ));
        }

        // Publication counts as having happened only when evidence says so.
        match self
            .deployment
            .deployment_exists(&profile_id, &target_relative_path)
        {
            Ok(true) => {}
            Ok(false) => {
                let cause = AppError::filesystem(
                    "The published deployment is missing after a successful publication",
                    format!(
                        "Profile {} has no folder {}",
                        profile_id, target_relative_path
                    ),
                );
                return Err(self.enter_recovery(
                    op,
                    "DEPLOYMENT_EVIDENCE_MISSING",
                    "The deployment could not be proven live after publication",
                    &cause,
                ));
            }
            Err(evidence_error) => {
                return Err(self.enter_recovery(
                    op,
                    "DEPLOYMENT_EVIDENCE_UNREADABLE",
                    "The published deployment could not be verified",
                    &evidence_error,
                ));
            }
        }

        self.lifecycle.complete_step(
            &op.id,
            INSTALL_STEP_PUBLISH_DEPLOYMENT,
            OperationStepKind::PublishDeployment,
            serde_json::json!({
                "profile_id": profile_id.to_string(),
                "mod_folder_name": target_relative_path,
                "deployment_id": deployment_id.to_string(),
            }),
        )?;

        // Step 5: the atomic database commit.
        self.lifecycle.start_step(
            &op.id,
            INSTALL_STEP_COMMIT_DATABASE,
            OperationStepKind::CommitInstallDatabase,
            serde_json::json!({
                "expected_profile_revision": op.expected_profile_revision,
                "deployment_rel_path": target_relative_path,
            }),
        )?;

        let commit =
            self.build_install_commit(op, profile, deployment_id, target_relative_path.clone())?;

        if let Err(error) = self.mutation_store.commit_install(commit) {
            // The folder is live but the database knows nothing about it: move it
            // into the recovery tree so the two sides agree again.
            self.lifecycle.start_step(
                &op.id,
                INSTALL_STEP_QUARANTINE_PUBLISHED,
                OperationStepKind::QuarantinePublishedDeployment,
                serde_json::json!({ "deployment_rel_path": target_relative_path }),
            )?;
            match self
                .deployment
                .quarantine_deployment(&profile_id, &op.id, &target_relative_path)
            {
                Ok(_) => {
                    self.lifecycle.complete_step(
                        &op.id,
                        INSTALL_STEP_QUARANTINE_PUBLISHED,
                        OperationStepKind::QuarantinePublishedDeployment,
                        serde_json::json!({ "deployment_rel_path": target_relative_path }),
                    )?;
                    self.lifecycle.fail_step(
                        &op.id,
                        INSTALL_STEP_COMMIT_DATABASE,
                        Some(error.to_string()),
                    )?;
                    self.lifecycle.transition(
                        &op.id,
                        OperationState::Failed,
                        Some("COMMIT_FAILED"),
                        Some(error.summary.clone()),
                    )?;
                    return Err(error);
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

        // The atomic transaction already recorded the commit step and moved the
        // operation to Succeeded, in one persistence boundary.

        // Step 6: staging is disposable once the installation is durably
        // committed, so a failed cleanup is not an operation failure - but the
        // journal must not claim it completed when it did not.
        self.lifecycle.start_step(
            &op.id,
            INSTALL_STEP_CLEANUP_STAGING,
            OperationStepKind::CleanupStaging,
            serde_json::json!({}),
        )?;
        self.finish_install_cleanup(op, &profile_id)?;

        let updated = self
            .operation_repo
            .get_operation(&op.id)?
            .ok_or_else(|| AppError::internal("Operation disappeared", op.id.to_string()))?;
        Ok(Self::op_to_dto(&updated))
    }
    /// The v2 removal execution lifecycle.
    ///
    /// `quarantine_deployment` -> `commit_removal_database`, with
    /// `restore_quarantined_deployment` persisted only when the commit fails
    /// after the folder actually left the profile.
    fn execute_removal_commit(&self, op: &Operation, profile: &Profile) -> AppResult<OperationDto> {
        // The same strict decoder recovery uses: this cannot fail for an
        // operation that passed preflight, and it never silently drops an id.
        let RemovalPlan {
            deployment_id,
            deployment_rel_path,
            removed_profile_component_ids,
        } = RemovalPlan::parse(op).map_err(AppError::removal_plan_invalid)?;

        let effects = removed_profile_component_ids
            .iter()
            .map(|comp_id| OperationEffect {
                id: uuid::Uuid::new_v4().to_string(),
                operation_id: op.id,
                profile_id: Some(profile.id),
                entity_type: "profile_component".to_string(),
                entity_id: comp_id.to_string(),
                change_kind: "ProfileComponentRemoved".to_string(),
                before_json: None,
                after_json: None,
                occurred_at: Utc::now(),
            })
            .collect();

        let profile_id = profile.id;

        // Step 1: move the deployment into the trusted recovery tree.
        self.lifecycle.start_step(
            &op.id,
            REMOVAL_STEP_QUARANTINE_DEPLOYMENT,
            OperationStepKind::QuarantineDeployment,
            serde_json::json!({ "deployment_rel_path": deployment_rel_path }),
        )?;

        if let Err(error) =
            self.deployment
                .quarantine_deployment(&profile_id, &op.id, &deployment_rel_path)
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

        // The step completes only on evidence: live gone, recovery copy present.
        let live = match self
            .deployment
            .deployment_exists(&profile_id, &deployment_rel_path)
        {
            Ok(live) => live,
            Err(evidence_error) => {
                return Err(self.enter_recovery(
                    op,
                    "QUARANTINE_EVIDENCE_UNREADABLE",
                    "The removed deployment could not be verified",
                    &evidence_error,
                ));
            }
        };
        let quarantined = match self.deployment.recovery_deployment_exists(
            &profile_id,
            &op.id,
            &deployment_rel_path,
        ) {
            Ok(quarantined) => quarantined,
            Err(evidence_error) => {
                return Err(self.enter_recovery(
                    op,
                    "QUARANTINE_EVIDENCE_UNREADABLE",
                    "The quarantined deployment could not be verified",
                    &evidence_error,
                ));
            }
        };
        if live || !quarantined {
            let cause = AppError::filesystem(
                "The deployment is not in the state a completed quarantine implies",
                format!(
                    "live_present={}, recovery_present={} for {}",
                    live, quarantined, deployment_rel_path
                ),
            );
            return Err(self.enter_recovery(
                op,
                "QUARANTINE_EVIDENCE_INCONSISTENT",
                "The deployment is neither fully removed nor fully recovered",
                &cause,
            ));
        }

        self.lifecycle.complete_step(
            &op.id,
            REMOVAL_STEP_QUARANTINE_DEPLOYMENT,
            OperationStepKind::QuarantineDeployment,
            serde_json::json!({ "deployment_rel_path": deployment_rel_path }),
        )?;

        // Step 2: the atomic removal commit.
        self.lifecycle.start_step(
            &op.id,
            REMOVAL_STEP_COMMIT_DATABASE,
            OperationStepKind::CommitRemovalDatabase,
            serde_json::json!({
                "expected_profile_revision": op.expected_profile_revision,
                "deployment_rel_path": deployment_rel_path,
            }),
        )?;

        if let Err(error) = self.mutation_store.commit_removal(RemovalCommit {
            operation_id: op.id,
            profile_id,
            expected_profile_revision: op.expected_profile_revision.unwrap_or(profile.revision),
            deployment_id,
            removed_profile_component_ids,
            effects,
            commit_step: self.removal_commit_step(op, &deployment_rel_path),
        }) {
            // The database still considers the deployment present, so put the
            // quarantined folder back where it was.
            self.lifecycle.start_step(
                &op.id,
                REMOVAL_STEP_RESTORE_QUARANTINED,
                OperationStepKind::RestoreQuarantinedDeployment,
                serde_json::json!({ "deployment_rel_path": deployment_rel_path }),
            )?;
            match self.deployment.restore_quarantined_deployment(
                &profile_id,
                &op.id,
                &deployment_rel_path,
            ) {
                Ok(()) => {
                    self.lifecycle.complete_step(
                        &op.id,
                        REMOVAL_STEP_RESTORE_QUARANTINED,
                        OperationStepKind::RestoreQuarantinedDeployment,
                        serde_json::json!({ "deployment_rel_path": deployment_rel_path }),
                    )?;
                    self.lifecycle.fail_step(
                        &op.id,
                        REMOVAL_STEP_COMMIT_DATABASE,
                        Some(error.to_string()),
                    )?;
                    self.lifecycle.transition(
                        &op.id,
                        OperationState::Failed,
                        Some("COMMIT_FAILED"),
                        Some(error.summary.clone()),
                    )?;
                    return Err(error);
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

        // The atomic transaction already recorded the commit step and moved the
        // operation to Succeeded, in one persistence boundary.

        let updated = self
            .operation_repo
            .get_operation(&op.id)?
            .ok_or_else(|| AppError::internal("Operation disappeared", op.id.to_string()))?;
        Ok(Self::op_to_dto(&updated))
    }

    /// The journal entry the atomic install commit completes.
    pub(crate) fn install_commit_step(&self, op: &Operation, target: &str) -> CommitStep {
        CommitStep {
            index: INSTALL_STEP_COMMIT_DATABASE,
            kind: OperationStepKind::CommitInstallDatabase
                .as_str()
                .to_string(),
            payload_json: serde_json::json!({
                "expected_profile_revision": op.expected_profile_revision,
                "deployment_rel_path": target,
            })
            .to_string(),
        }
    }

    /// The journal entry the atomic removal commit completes.
    pub(crate) fn removal_commit_step(&self, op: &Operation, target: &str) -> CommitStep {
        CommitStep {
            index: REMOVAL_STEP_COMMIT_DATABASE,
            kind: OperationStepKind::CommitRemovalDatabase
                .as_str()
                .to_string(),
            payload_json: serde_json::json!({
                "expected_profile_revision": op.expected_profile_revision,
                "deployment_rel_path": target,
            })
            .to_string(),
        }
    }

    /// Removes the staging tree and records what actually happened.
    ///
    /// Cleanup is housekeeping: it runs after the installation is durably
    /// committed, so a failure must not turn a successful install into a failed
    /// operation. It also must not be reported as completed when it was not.
    pub(crate) fn finish_install_cleanup(
        &self,
        op: &Operation,
        profile_id: &ProfileId,
    ) -> AppResult<()> {
        match self.staging.clean_staging_dir(profile_id, &op.id) {
            Ok(()) => self.lifecycle.complete_step(
                &op.id,
                INSTALL_STEP_CLEANUP_STAGING,
                OperationStepKind::CleanupStaging,
                serde_json::json!({ "cleaned": true }),
            ),
            Err(error) => {
                // Recovery may be recording the outcome of a cleanup that never
                // started, so make sure the entry exists before failing it.
                let started = self
                    .lifecycle
                    .load_steps(&op.id)?
                    .iter()
                    .any(|step| step.step_index == INSTALL_STEP_CLEANUP_STAGING);
                if !started {
                    self.lifecycle.start_step(
                        &op.id,
                        INSTALL_STEP_CLEANUP_STAGING,
                        OperationStepKind::CleanupStaging,
                        serde_json::json!({}),
                    )?;
                }
                self.lifecycle.fail_step(
                    &op.id,
                    INSTALL_STEP_CLEANUP_STAGING,
                    Some(error.to_string()),
                )
            }
        }
    }

    /// Records that an operation needs manual reconciliation and returns the
    /// error the caller must report.
    ///
    /// Persisting `RecoveryRequired` is itself authoritative. If that write
    /// fails, the returned error says so - it never pretends the state was
    /// durably recorded, and it keeps the original failure as context.
    pub(crate) fn enter_recovery(
        &self,
        op: &Operation,
        code: &str,
        summary: &str,
        cause: &AppError,
    ) -> AppError {
        let evidence = serde_json::json!({
            "code": code,
            "message": summary,
            "cause": cause.to_string(),
        })
        .to_string();

        match self.lifecycle.transition(
            &op.id,
            OperationState::RecoveryRequired,
            Some(code),
            Some(evidence),
        ) {
            // The diagnosis survives: code, summary and details still describe
            // what actually failed, while the category, recoverability and
            // operation id describe what the caller has to do about it.
            Ok(()) => cause.clone().into_recovery_required(op.id),
            Err(persist_error) => recovery_state_unknown(op.id, cause, &persist_error),
        }
    }

    pub(crate) fn op_to_dto(op: &Operation) -> OperationDto {
        let kind_str = match op.kind {
            OperationKind::SmapiSetup => "smapi_setup",
            OperationKind::ModInstall => "mod_install",
            OperationKind::ModRemove => "mod_remove",
            OperationKind::GameLaunch => "game_launch",
            OperationKind::ProfileCreate => "profile_create",
            OperationKind::ProfileDelete => "profile_delete",
        };

        let state_str = match op.state {
            OperationState::Draft => "draft",
            OperationState::Prepared => "prepared",
            OperationState::Running => "running",
            OperationState::Committing => "committing",
            OperationState::Succeeded => "succeeded",
            OperationState::Failed => "failed",
            OperationState::CancellationRequested => "cancellation_requested",
            OperationState::Cancelling => "cancelling",
            OperationState::Cancelled => "cancelled",
            OperationState::RollingBack => "rolling_back",
            OperationState::RolledBack => "rolled_back",
            OperationState::RecoveryRequired => "recovery_required",
        };

        OperationDto {
            id: op.id.to_string(),
            kind: kind_str.to_string(),
            state: state_str.to_string(),
            game_installation_id: op.game_installation_id.map(|id| id.to_string()),
            profile_id: op.profile_id.map(|id| id.to_string()),
            progress_current: op.progress_current,
            progress_total: op.progress_total,
            error_code: op.error_code.clone(),
            error_message: op.error_json.clone(),
            created_at: op.created_at.to_rfc3339(),
            updated_at: op.updated_at.to_rfc3339(),
            completed_at: op.completed_at.map(|t| t.to_rfc3339()),
        }
    }
}

/// The operation needs manual reconciliation, and recording that fact failed.
///
/// This is deliberately not reported as an ordinary failure: the caller has to
/// know that neither the state nor the error metadata is trustworthy, and which
/// operation is affected.
pub(crate) fn recovery_state_unknown(
    operation_id: OperationId,
    cause: &AppError,
    persist_error: &AppError,
) -> AppError {
    AppError::recovery_state_persist_failed(
        operation_id,
        format!(
            "operation {}: {}; persistence failure: {}",
            operation_id, cause, persist_error
        ),
    )
}
