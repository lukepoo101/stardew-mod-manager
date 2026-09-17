use crate::api::dto::OperationDto;
use crate::error::{AppError, AppResult};
use crate::ports::deployment::{DeploymentPort, StagedContentVerifierPort, StagingPort};
use crate::ports::launcher::GameLauncherPort;
use crate::ports::repositories::{
    AtomicMutationStore, DeploymentRepository, InstallCommit, OperationRepository,
    PackageCatalogRepository, ProfileRepository, RemovalCommit,
};
use crate::ports::runtime::SmapiInspectorPort;
use chrono::Utc;
use manager_core::deployment::{
    DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
};
use manager_core::ids::{ArtifactHash, DeploymentId, OperationId, ProfileComponentId, ProfileId};
use manager_core::install::InstallPlan;
use manager_core::operation::{Operation, OperationEffect, OperationKind, OperationState};
use manager_core::package::PackageComponent;
use manager_core::ports::InstanceLock;
use manager_core::smapi::SmapiObservation;
use std::str::FromStr;
use std::sync::Arc;

const SMAPI_RECOVERY_VERSION_UNKNOWN: &str = "SMAPI_RECOVERY_VERSION_UNKNOWN";
const LEGACY_OPERATION_REQUIRES_RECONCILIATION: &str = "LEGACY_OPERATION_REQUIRES_RECONCILIATION";

fn smapi_recovery_failure(observation: &SmapiObservation) -> Option<(&'static str, &'static str)> {
    if observation.is_present && observation.observed_version.is_none() {
        Some((
            SMAPI_RECOVERY_VERSION_UNKNOWN,
            "SMAPI installation evidence is present, but its version could not be determined; repair or rerun setup",
        ))
    } else {
        None
    }
}

fn recovery_requires_instance_guard(profile_scoped: bool, kind: OperationKind) -> bool {
    profile_scoped || kind == OperationKind::SmapiSetup
}

#[allow(clippy::result_large_err)]
fn ensure_profile_write_available(
    operation_repo: &dyn OperationRepository,
    profile_id: &ProfileId,
    operation_id: &OperationId,
) -> AppResult<()> {
    for resource in operation_repo.list_unresolved_resources_for_profile(profile_id)? {
        if resource.access_mode == manager_core::operation::AccessMode::Write
            && resource.operation_id != *operation_id
        {
            return Err(AppError::conflict(
                "PROFILE_OPERATION_UNRESOLVED",
                format!(
                    "Profile {} has unresolved operation {}; reconcile it before mutating the profile",
                    profile_id, resource.operation_id
                ),
            ));
        }
    }
    Ok(())
}

pub struct OperationsService {
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
    game_repo: Arc<dyn crate::ports::repositories::GameInstallationRepository>,
    smapi_inspector: Arc<dyn SmapiInspectorPort>,
    smapi_repo: Arc<dyn crate::ports::repositories::SmapiRepository>,
}

impl OperationsService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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
        game_repo: Arc<dyn crate::ports::repositories::GameInstallationRepository>,
        smapi_inspector: Arc<dyn SmapiInspectorPort>,
        smapi_repo: Arc<dyn crate::ports::repositories::SmapiRepository>,
    ) -> Self {
        Self {
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

    pub fn cancel_operation(&self, id: &OperationId) -> AppResult<()> {
        let op = self
            .operation_repo
            .get_operation(id)?
            .ok_or_else(|| AppError::validation("OPERATION_NOT_FOUND", "Operation not found"))?;

        if op.state == OperationState::Draft || op.state == OperationState::Prepared {
            if let Some(pid) = op.profile_id {
                let _ = self.staging.clean_staging_dir(&pid, id);
            }
            self.operation_repo.update_operation_state(
                id,
                OperationState::Cancelled,
                None,
                Some("Cancelled by user".to_string()),
            )?;
        } else {
            return Err(AppError::conflict(
                "OPERATION_NOT_CANCELLABLE",
                format!("Operation {} is already in the mutation lifecycle", id),
            ));
        }
        Ok(())
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

        // Stale-plan protection: revalidate profile revision
        let profile_id = op
            .profile_id
            .ok_or_else(|| AppError::internal("Operation lacks profile ID", id.to_string()))?;
        ensure_profile_write_available(&*self.operation_repo, &profile_id, id)?;
        let _mutation_guard = self
            .instance_lock
            .acquire_guard()
            .map_err(|e| AppError::conflict("INSTANCE_LOCKED", e))?;
        if self.launcher.is_game_running(None) {
            return Err(AppError::conflict(
                "GAME_RUNNING",
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
                return Err(AppError::conflict(
                    "Profile was modified since preview was generated",
                    format!(
                        "Expected profile revision {}, but current revision is {}",
                        expected_rev, profile.revision
                    ),
                ));
            }
        }

        if op.kind == OperationKind::ModInstall {
            let plan: InstallPlan = serde_json::from_str(&op.plan_json)
                .map_err(|e| AppError::internal("Corrupted install plan", e.to_string()))?;
            if !plan.dependency_report.is_installable {
                return Err(AppError::validation(
                    "INSTALL_BLOCKED",
                    "Resolve the installation blockers before installing",
                ));
            }
        }

        // Persist the validated preflight boundary before entering mutation.
        if op.state == OperationState::Draft {
            self.operation_repo
                .update_operation_state(id, OperationState::Prepared, None, None)?;
            op.state = OperationState::Prepared;
        }

        // Transition through the explicit committing boundary before any
        // filesystem/database mutation begins. Atomic mutation commits finish
        // the Committing -> Succeeded transition in the same DB transaction.
        self.operation_repo
            .update_operation_state(id, OperationState::Running, None, None)?;
        self.operation_repo
            .update_operation_state(id, OperationState::Committing, None, None)?;
        op.state = OperationState::Committing;

        let result = match op.kind {
            OperationKind::ModInstall => self.execute_install_commit(&op, &profile),
            OperationKind::ModRemove => self.execute_removal_commit(&op, &profile),
            _ => Err(AppError::validation(
                "UNSUPPORTED_OPERATION_KIND",
                format!("Cannot commit operation kind {:?}", op.kind),
            )),
        };

        if result.is_err() {
            if let Ok(Some(current)) = self.operation_repo.get_operation(id) {
                if matches!(
                    current.state,
                    OperationState::Running
                        | OperationState::Committing
                        | OperationState::RollingBack
                        | OperationState::Cancelling
                ) {
                    let _ = self.operation_repo.update_operation_state(
                        id,
                        OperationState::RecoveryRequired,
                        Some("EXECUTION_INTERRUPTED".to_string()),
                        Some("Operation failed after entering the mutation phase; reconciliation is required".to_string()),
                    );
                }
            }
        }
        result
    }

    fn execute_install_commit(
        &self,
        op: &Operation,
        profile: &manager_core::profile::Profile,
    ) -> AppResult<OperationDto> {
        let plan: InstallPlan = serde_json::from_str(&op.plan_json)
            .map_err(|e| AppError::internal("Corrupted install plan JSON", e.to_string()))?;

        let artifact_hash = ArtifactHash::parse(&plan.package_hash)
            .map_err(|e| AppError::validation("INVALID_ARTIFACT_HASH", e.to_string()))?;
        let artifact = self
            .package_repo
            .get_artifact(&artifact_hash)?
            .ok_or_else(|| {
                AppError::internal("Artifact record missing", artifact_hash.to_string())
            })?;

        let acquisitions = self
            .package_repo
            .get_acquisitions_for_artifact(&artifact_hash)?;
        let acquisition = acquisitions.into_iter().next().ok_or_else(|| {
            AppError::internal("Acquisition record missing", artifact_hash.to_string())
        })?;

        let staging_root = self.staging.create_staging_dir(&profile.id, &op.id)?;
        let staged_content_dir = staging_root.join(&plan.mod_folder_name);

        // Step 4: Verify staged content
        if let Err(e) = self
            .staging_verifier
            .verify_staged(&plan, &staged_content_dir)
        {
            let _ = self.operation_repo.update_operation_state(
                &op.id,
                OperationState::Failed,
                Some("VERIFICATION_FAILED".to_string()),
                Some(e.to_string()),
            );
            return Err(e);
        }

        // Step 5: Publish deployment to profile Mods folder
        let deployment_id = DeploymentId::new();
        let target_relative_path = plan.mod_folder_name.clone();
        if let Err(e) = self.deployment.publish_deployment(
            &profile.id,
            &staged_content_dir,
            &target_relative_path,
        ) {
            let target_exists = self
                .deployment
                .deployment_exists(&profile.id, &target_relative_path)
                .unwrap_or(false);
            let failure_state = if e.code == "DEPLOYMENT_DESTINATION_EXISTS" {
                OperationState::Failed
            } else if target_exists {
                OperationState::RecoveryRequired
            } else {
                OperationState::Failed
            };
            let _ = self.operation_repo.update_operation_state(
                &op.id,
                failure_state,
                Some("DEPLOYMENT_FAILED".to_string()),
                Some(e.summary.clone()),
            );
            return Err(e);
        }

        // Prepare package components and profile components
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
            let pkg_comp = PackageComponent {
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
            };
            package_components.push(pkg_comp);

            let prof_comp_id = ProfileComponentId::new();
            let prof_comp = ProfileComponent {
                id: prof_comp_id,
                profile_id: profile.id,
                deployment_id,
                package_component_id: comp_id,
                enabled: true,
                installed_reason: InstalledReason::Direct,
            };
            profile_components.push(prof_comp);

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
                let pkg_comp = PackageComponent {
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
                };
                package_components.push(pkg_comp);

                let prof_comp_id = ProfileComponentId::new();
                let prof_comp = ProfileComponent {
                    id: prof_comp_id,
                    profile_id: profile.id,
                    deployment_id,
                    package_component_id: comp_id,
                    enabled: true,
                    installed_reason: InstalledReason::BundleCompanion,
                };
                profile_components.push(prof_comp);

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

        let deployment_record = ProfileDeployment {
            id: deployment_id,
            profile_id: profile.id,
            artifact_hash,
            root_relative_path: target_relative_path,
            installed_at: Utc::now(),
            state: DeploymentState::Present,
        };

        // Step 6: Atomic commit to database
        if let Err(e) = self.mutation_store.commit_install(InstallCommit {
            operation_id: op.id,
            profile_id: profile.id,
            expected_profile_revision: op.expected_profile_revision.unwrap_or(profile.revision),
            artifact,
            acquisition,
            package_components,
            deployment: deployment_record,
            profile_components,
            effects,
        }) {
            // The folder is already live in the profile but the database knows nothing
            // about it. Move it into the recovery tree so the two sides agree again.
            let rolled_back = self
                .deployment
                .quarantine_deployment(&profile.id, &op.id, &plan.mod_folder_name)
                .is_ok();
            let _ = self.operation_repo.update_operation_state(
                &op.id,
                if rolled_back {
                    OperationState::Failed
                } else {
                    OperationState::RecoveryRequired
                },
                Some("COMMIT_FAILED".to_string()),
                Some(e.summary.clone()),
            );
            return Err(e);
        }

        // Step 7: Clean up staging
        let _ = self.staging.clean_staging_dir(&profile.id, &op.id);

        // Step 8: Mark Succeeded
        self.operation_repo.update_operation_state(
            &op.id,
            OperationState::Succeeded,
            None,
            None,
        )?;

        let updated = self.operation_repo.get_operation(&op.id)?.unwrap();
        Ok(Self::op_to_dto(&updated))
    }

    fn execute_removal_commit(
        &self,
        op: &Operation,
        profile: &manager_core::profile::Profile,
    ) -> AppResult<OperationDto> {
        let val: serde_json::Value = serde_json::from_str(&op.plan_json)
            .map_err(|e| AppError::internal("Corrupted removal plan JSON", e.to_string()))?;

        let deployment_id_str = val
            .get("deployment_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("Missing deployment_id in removal plan", ""))?;
        let deployment_id = DeploymentId::from_str(deployment_id_str)
            .map_err(|e| AppError::internal("Invalid deployment_id UUID", e.to_string()))?;

        let deployment_rel_path = val
            .get("deployment_rel_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("Missing deployment_rel_path in removal plan", ""))?;

        let removed_ids_val = val
            .get("removed_profile_component_ids")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                AppError::internal("Missing removed_profile_component_ids in removal plan", "")
            })?;

        let mut removed_profile_component_ids = Vec::new();
        let mut effects = Vec::new();

        for item in removed_ids_val {
            if let Some(id_str) = item.as_str() {
                if let Ok(comp_id) = ProfileComponentId::from_str(id_str) {
                    removed_profile_component_ids.push(comp_id);
                    effects.push(OperationEffect {
                        id: uuid::Uuid::new_v4().to_string(),
                        operation_id: op.id,
                        profile_id: Some(profile.id),
                        entity_type: "profile_component".to_string(),
                        entity_id: comp_id.to_string(),
                        change_kind: "ProfileComponentRemoved".to_string(),
                        before_json: None,
                        after_json: None,
                        occurred_at: Utc::now(),
                    });
                }
            }
        }

        // Quarantine deployment folder to recovery tree
        if let Err(e) =
            self.deployment
                .quarantine_deployment(&profile.id, &op.id, deployment_rel_path)
        {
            let _ = self.operation_repo.update_operation_state(
                &op.id,
                OperationState::RecoveryRequired,
                Some("QUARANTINE_FAILED".to_string()),
                Some(e.summary.clone()),
            );
            return Err(e);
        }

        // Commit database removal
        if let Err(e) = self.mutation_store.commit_removal(RemovalCommit {
            operation_id: op.id,
            profile_id: profile.id,
            expected_profile_revision: op.expected_profile_revision.unwrap_or(profile.revision),
            deployment_id,
            removed_profile_component_ids,
            effects,
        }) {
            // The database still considers the deployment present, so put the
            // quarantined folder back where it was.
            let restored = self
                .deployment
                .restore_quarantined_deployment(&profile.id, &op.id, deployment_rel_path)
                .is_ok();
            let _ = self.operation_repo.update_operation_state(
                &op.id,
                if restored {
                    OperationState::Failed
                } else {
                    OperationState::RecoveryRequired
                },
                Some("COMMIT_FAILED".to_string()),
                Some(e.summary.clone()),
            );
            return Err(e);
        }

        self.operation_repo.update_operation_state(
            &op.id,
            OperationState::Succeeded,
            None,
            None,
        )?;

        let updated = self.operation_repo.get_operation(&op.id)?.unwrap();
        Ok(Self::op_to_dto(&updated))
    }

    pub fn retry_recovery(&self) -> AppResult<()> {
        let unresolved = self.operation_repo.list_unresolved_operations()?;
        let needs_instance_guard = unresolved
            .iter()
            .any(|op| recovery_requires_instance_guard(op.profile_id.is_some(), op.kind));
        let _mutation_guard = if needs_instance_guard {
            Some(
                self.instance_lock
                    .acquire_guard()
                    .map_err(|e| AppError::conflict("INSTANCE_LOCKED", e))?,
            )
        } else {
            None
        };
        if needs_instance_guard && self.launcher.is_game_running(None) {
            return Err(AppError::conflict(
                "GAME_RUNNING",
                "Stop Stardew Valley before reconciling managed files",
            ));
        }
        for op in unresolved {
            if matches!(op.state, OperationState::Draft | OperationState::Prepared) {
                if let Some(pid) = op.profile_id {
                    let _ = self.staging.clean_staging_dir(&pid, &op.id);
                }
                self.operation_repo.update_operation_state(
                    &op.id,
                    OperationState::Cancelled,
                    Some("PREVIEW_EXPIRED".to_string()),
                    Some("Uncommitted preview was cancelled during startup recovery".to_string()),
                )?;
            } else if op.error_code.as_deref() == Some(LEGACY_OPERATION_REQUIRES_RECONCILIATION)
                && matches!(
                    op.kind,
                    OperationKind::ModInstall | OperationKind::ModRemove
                )
            {
                // Migration 0007 retains the old operation journal, but its plan
                // does not contain the modern commit payload. Reconcile these
                // operations using the old filesystem-first contract instead of
                // sending them through the normal modern commit path.
                match op.kind {
                    OperationKind::ModInstall => {
                        self.reconcile_legacy_install(&op)?;
                    }
                    OperationKind::ModRemove => {
                        self.reconcile_legacy_removal(&op)?;
                    }
                    _ => unreachable!(),
                }
            } else if op.state.requires_recovery() && op.kind == OperationKind::SmapiSetup {
                let game_id = op.game_installation_id.ok_or_else(|| {
                    AppError::internal("SMAPI recovery lacks game ID", op.id.to_string())
                })?;
                let game = self
                    .game_repo
                    .get_game(&game_id)?
                    .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game not found"))?;
                let observation = self.smapi_inspector.observe_smapi(&game.canonical_root)?;
                if observation.is_present {
                    if let Some(version) = observation.observed_version {
                        let plan: serde_json::Value =
                            serde_json::from_str(&op.plan_json).map_err(|e| {
                                AppError::internal("Corrupted SMAPI recovery plan", e.to_string())
                            })?;
                        let policy_id = plan
                            .get("release_policy_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("recovered")
                            .to_string();
                        self.smapi_repo.save_smapi_installation(
                            &manager_core::smapi::ManagedSmapiInstallation {
                                game_installation_id: game_id,
                                release_version: version,
                                release_policy_id: policy_id,
                                installed_at: observation.observed_at,
                            },
                        )?;
                        self.operation_repo.update_operation_state(
                            &op.id,
                            OperationState::Succeeded,
                            Some("RECOVERED_SMAPI_STATE".to_string()),
                            Some(
                                "Recovered managed SMAPI state from filesystem evidence"
                                    .to_string(),
                            ),
                        )?;
                    } else if let Some((error_code, message)) = smapi_recovery_failure(&observation)
                    {
                        self.operation_repo.update_operation_state(
                            &op.id,
                            OperationState::Failed,
                            Some(error_code.to_string()),
                            Some(message.to_string()),
                        )?;
                    }
                } else {
                    self.operation_repo.update_operation_state(
                        &op.id,
                        OperationState::Failed,
                        Some("SMAPI_NOT_PRESENT".to_string()),
                        Some("SMAPI was not present after the interrupted setup".to_string()),
                    )?;
                }
            } else if op.state.requires_recovery()
                && op.profile_id.is_some()
                && matches!(
                    op.kind,
                    OperationKind::ModInstall | OperationKind::ModRemove
                )
            {
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

                recovery_result?;
                self.operation_repo.update_operation_state(
                    &op.id,
                    OperationState::Failed,
                    Some("RECOVERED_TO_TERMINAL_STATE".to_string()),
                    Some("Filesystem evidence was conservatively reconciled; retry with a fresh plan".to_string()),
                )?;
            } else if op.state.requires_recovery()
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
                self.operation_repo.update_operation_state(
                    &op.id,
                    OperationState::RecoveryRequired,
                    Some("RECONCILIATION_REQUIRED".to_string()),
                    Some("Interrupted operation requires filesystem reconciliation; recovery files were preserved".to_string()),
                )?;
            }
        }
        Ok(())
    }

    fn reconcile_legacy_install(&self, op: &Operation) -> AppResult<()> {
        let profile_id = op.profile_id.ok_or_else(|| {
            AppError::internal(
                "Legacy install recovery lacks profile ID",
                op.id.to_string(),
            )
        })?;
        let plan: serde_json::Value = serde_json::from_str(&op.plan_json)
            .map_err(|e| AppError::internal("Corrupted legacy install plan", e.to_string()))?;
        let folder = plan
            .get("mod_folder_name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                AppError::internal(
                    "Legacy install plan lacks deployment path",
                    op.id.to_string(),
                )
            })?;
        manager_core::install::validate_relative_path(folder)
            .map_err(|e| AppError::internal("Invalid legacy install path", e))?;

        // The old runtime could have published the folder before its database
        // transaction committed. The safe compatibility outcome is to quarantine
        // that unowned folder and finish the operation as failed; this restores
        // the invariant that every live deployment has a database owner.
        if self.deployment.deployment_exists(&profile_id, folder)? {
            if let Err(error) = self
                .deployment
                .quarantine_deployment(&profile_id, &op.id, folder)
            {
                return self.retain_legacy_recovery(
                    op,
                    "LEGACY_INSTALL_RECONCILIATION_FAILED",
                    &error.to_string(),
                );
            }
        }

        self.operation_repo.update_operation_state(
            &op.id,
            OperationState::Failed,
            Some("LEGACY_INSTALL_RECONCILED".to_string()),
            Some(
                "Legacy installation evidence was removed from the live profile; retry with a fresh plan"
                    .to_string(),
            ),
        )?;
        Ok(())
    }

    fn reconcile_legacy_removal(&self, op: &Operation) -> AppResult<()> {
        let profile_id = op.profile_id.ok_or_else(|| {
            AppError::internal(
                "Legacy removal recovery lacks profile ID",
                op.id.to_string(),
            )
        })?;
        let plan: serde_json::Value = serde_json::from_str(&op.plan_json)
            .map_err(|e| AppError::internal("Corrupted legacy removal plan", e.to_string()))?;
        let folder = plan
            .get("deployment_rel_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                AppError::internal(
                    "Legacy removal plan lacks deployment path",
                    op.id.to_string(),
                )
            })?;
        manager_core::install::validate_relative_path(folder)
            .map_err(|e| AppError::internal("Invalid legacy removal path", e))?;

        let source_exists = self.deployment.deployment_exists(&profile_id, folder)?;
        let recovery_exists =
            self.deployment
                .recovery_deployment_exists(&profile_id, &op.id, folder)?;
        if source_exists && recovery_exists {
            return self.retain_legacy_recovery(
                op,
                "LEGACY_REMOVAL_CONFLICT",
                "Both the live and recovery deployment exist; manual inspection is required",
            );
        }
        if source_exists || !recovery_exists {
            if source_exists {
                return self.finish_legacy_removal_as_failed(
                    op,
                    "LEGACY_REMOVAL_NOT_PUBLISHED",
                    "Legacy removal did not move the deployment; it remains installed",
                );
            }
            return self.retain_legacy_recovery(
                op,
                "LEGACY_REMOVAL_EVIDENCE_MISSING",
                "Legacy removal evidence is missing from both live and recovery storage",
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
                    .map_err(|e| AppError::internal("Invalid legacy component ID", e.to_string()))
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
                return self.retain_legacy_recovery(
                    op,
                    "LEGACY_REMOVAL_DATABASE_STATE_UNRECONCILED",
                    "Legacy removal has no matching profile components, but its deployment is not marked quarantined",
                );
            }
            self.operation_repo.update_operation_state(
                &op.id,
                OperationState::Succeeded,
                Some("LEGACY_REMOVAL_ALREADY_COMMITTED".to_string()),
                Some("Legacy removal database state was already committed".to_string()),
            )?;
            return Ok(());
        }

        let deployment_id = selected[0].deployment_id;
        if selected
            .iter()
            .any(|component| component.deployment_id != deployment_id)
        {
            return self.retain_legacy_recovery(
                op,
                "LEGACY_REMOVAL_MULTIPLE_DEPLOYMENTS",
                "Legacy removal references components from multiple deployments",
            );
        }
        let profile = self
            .profile_repo
            .get_profile(&profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        let removed_profile_component_ids = selected.iter().map(|component| component.id).collect();

        // The atomic mutation store deliberately only accepts commits from its
        // committing boundary. A migrated legacy journal starts at
        // recovery_required, so enter that boundary explicitly before applying
        // the old removal transaction.
        self.operation_repo.update_operation_state(
            &op.id,
            OperationState::Committing,
            Some("LEGACY_REMOVAL_COMMITTING".to_string()),
            None,
        )?;
        if let Err(error) = self.mutation_store.commit_removal(RemovalCommit {
            operation_id: op.id,
            profile_id,
            expected_profile_revision: profile.revision,
            deployment_id,
            removed_profile_component_ids,
            effects: Vec::<OperationEffect>::new(),
        }) {
            return self.retain_legacy_recovery(
                op,
                "LEGACY_REMOVAL_DATABASE_RECONCILIATION_FAILED",
                &error.to_string(),
            );
        }
        self.operation_repo.update_operation_metadata(
            &op.id,
            Some("LEGACY_REMOVAL_RECONCILED".to_string()),
            Some("Legacy removal filesystem and database state were reconciled".to_string()),
        )?;
        Ok(())
    }

    fn finish_legacy_removal_as_failed(
        &self,
        op: &Operation,
        code: &str,
        message: &str,
    ) -> AppResult<()> {
        self.operation_repo.update_operation_state(
            &op.id,
            OperationState::Failed,
            Some(code.to_string()),
            Some(message.to_string()),
        )?;
        Ok(())
    }

    fn retain_legacy_recovery(&self, op: &Operation, code: &str, message: &str) -> AppResult<()> {
        self.operation_repo.update_operation_state(
            &op.id,
            OperationState::RecoveryRequired,
            Some(code.to_string()),
            Some(message.to_string()),
        )?;
        self.operation_repo.update_operation_metadata(
            &op.id,
            Some(code.to_string()),
            Some(message.to_string()),
        )?;
        // This is a recovery situation, not an unexpected implementation
        // failure: the operation is unresolved, a human has to reconcile it, and
        // the frontend needs the operation id to route to it. Category,
        // recoverability and operation id therefore cross IPC intact; the
        // operation state and evidence above are untouched by the error shape.
        Err(
            AppError::recovery_required(code, "Legacy recovery reconciliation failed", op.id)
                .with_details(format!("{}: {}", code, message)),
        )
    }

    fn op_to_dto(op: &Operation) -> OperationDto {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn smapi_recovery_unknown_version_is_terminal_failure() {
        let observation = SmapiObservation {
            is_present: true,
            observed_version: None,
            executable_present: true,
            evidence: vec!["SMAPI executable and internal files present".to_string()],
            observed_at: Utc::now(),
        };

        assert_eq!(
            smapi_recovery_failure(&observation),
            Some((
                "SMAPI_RECOVERY_VERSION_UNKNOWN",
                "SMAPI installation evidence is present, but its version could not be determined; repair or rerun setup",
            ))
        );
    }

    #[test]
    fn smapi_recovery_requires_instance_guard_without_profile() {
        assert!(recovery_requires_instance_guard(
            false,
            OperationKind::SmapiSetup
        ));
        assert!(recovery_requires_instance_guard(
            true,
            OperationKind::ModInstall
        ));
        assert!(!recovery_requires_instance_guard(
            false,
            OperationKind::ProfileCreate
        ));
    }
}
