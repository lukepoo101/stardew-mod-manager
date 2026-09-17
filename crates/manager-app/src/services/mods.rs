use crate::api::dto::{OperationPreviewDto, PackageComponentPreviewDto};
use crate::error::{AppError, AppResult};
use crate::ports::deployment::{ArchiveInspectorPort, StagedContentVerifierPort, StagingPort};
use crate::ports::repositories::{
    DeploymentRepository, OperationRepository, PackageCatalogRepository, ProfileRepository,
    SmapiRepository,
};
use crate::services::operation_lifecycle::OperationLifecycle;
use crate::services::packages::PackagesService;
use crate::services::resources::ensure_profile_write_available;
use chrono::Utc;
use manager_core::dependency::evaluation::build_dependency_graph;
use manager_core::ids::{OperationId, ProfileComponentId, ProfileId};
use manager_core::operation::{
    AccessMode, Operation, OperationKind, OperationResource, OperationState, OperationStepKind,
    ResourceKind, INSTALL_STEP_INSPECT_AND_STAGE, INSTALL_STEP_RETAIN_ARTIFACT,
    INSTALL_STEP_VERIFY_STAGED, OPERATION_PLAN_SCHEMA_V2,
};

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

pub struct ModsService {
    lifecycle: OperationLifecycle,
    packages: Arc<PackagesService>,
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    smapi_repo: Arc<dyn SmapiRepository>,
    archive_inspector: Arc<dyn ArchiveInspectorPort>,
    staging: Arc<dyn StagingPort>,
    staging_verifier: Arc<dyn StagedContentVerifierPort>,
}

impl ModsService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        packages: Arc<PackagesService>,
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        operation_repo: Arc<dyn OperationRepository>,
        smapi_repo: Arc<dyn SmapiRepository>,
        archive_inspector: Arc<dyn ArchiveInspectorPort>,
        staging: Arc<dyn StagingPort>,
        staging_verifier: Arc<dyn StagedContentVerifierPort>,
    ) -> Self {
        Self {
            lifecycle: OperationLifecycle::new(operation_repo.clone()),
            packages,
            profile_repo,
            deployment_repo,
            package_repo,
            operation_repo,
            smapi_repo,
            archive_inspector,
            staging,
            staging_verifier,
        }
    }

    pub fn prepare_install(
        &self,
        profile_id: &ProfileId,
        source_zip: &Path,
    ) -> AppResult<OperationPreviewDto> {
        let profile = self.profile_repo.get_profile(profile_id)?.ok_or_else(|| {
            AppError::validation(
                "PROFILE_NOT_FOUND",
                format!("Profile {} not found", profile_id),
            )
        })?;
        if profile.state != manager_core::profile::ProfileState::Active {
            return Err(AppError::validation(
                "PROFILE_NOT_ACTIVE",
                "Only active profiles can receive installations",
            ));
        }
        ensure_profile_write_available(&*self.operation_repo, profile_id, None)?;

        // 1. Authoritative retention of source bytes
        let (artifact, acquisition) = self.packages.retain_local_package(source_zip)?;
        let artifact_path = self.packages.get_artifact_path(&artifact.hash)?;

        let op_id = OperationId::new();
        let staging_dir = self.staging.create_staging_dir(profile_id, &op_id)?;

        // Collect existing installed manifests in profile
        let profile_comps = self.deployment_repo.list_profile_components(profile_id)?;
        let mut installed_manifests = Vec::new();
        for pc in &profile_comps {
            if let Some(comp) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            {
                installed_manifests.push((comp.unique_id, comp.version));
            }
        }

        // Get SMAPI observed version
        let smapi_version = self
            .smapi_repo
            .get_smapi_installation(&profile.game_installation_id)?
            .map(|s| s.release_version);

        // Inspect and stage
        let plan = match self.archive_inspector.inspect_and_stage(
            &artifact_path,
            &op_id,
            &staging_dir,
            &installed_manifests,
            smapi_version.as_deref(),
        ) {
            Ok(p) => p,
            Err(e) => {
                let _ = self.staging.clean_staging_dir(profile_id, &op_id);
                return Err(e);
            }
        };

        // Verify staging integrity
        if let Err(e) = self
            .staging_verifier
            .verify_staged(&plan, &staging_dir.join(&plan.mod_folder_name))
        {
            let _ = self.staging.clean_staging_dir(profile_id, &op_id);
            return Err(e);
        }

        let plan_json = serde_json::to_string(&plan)
            .map_err(|e| AppError::internal("Failed to serialize install plan", e.to_string()))?;

        // Persist operation in Draft state
        let op = Operation {
            id: op_id,
            kind: OperationKind::ModInstall,
            state: OperationState::Draft,
            game_installation_id: Some(profile.game_installation_id),
            profile_id: Some(*profile_id),
            expected_profile_revision: Some(profile.revision),
            plan_schema_version: OPERATION_PLAN_SCHEMA_V2,
            plan_json,
            progress_current: Some(3),
            progress_total: Some(6),
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        };
        self.operation_repo.create_operation(&op)?;

        // Persist operation resource scope
        self.operation_repo
            .save_operation_resource(&OperationResource {
                operation_id: op_id,
                resource_kind: ResourceKind::Profile,
                resource_id: profile_id.to_string(),
                access_mode: AccessMode::Write,
            })?;

        // Persist the completed preparation boundaries before the operation can
        // become Prepared, so a v2 plan always carries the evidence its recovery
        // needs. Only trusted, ID-derived evidence is recorded - never an
        // absolute managed path.
        self.lifecycle.complete_step(
            &op_id,
            INSTALL_STEP_RETAIN_ARTIFACT,
            OperationStepKind::RetainArtifact,
            serde_json::json!({
                "artifact_hash": artifact.hash.as_str(),
                "byte_size": artifact.byte_size,
            }),
        )?;
        self.lifecycle.complete_step(
            &op_id,
            INSTALL_STEP_INSPECT_AND_STAGE,
            OperationStepKind::InspectAndStage,
            serde_json::json!({
                "profile_id": profile_id.to_string(),
                "mod_folder_name": plan.mod_folder_name,
                "expected_profile_revision": profile.revision,
            }),
        )?;
        self.lifecycle.complete_step(
            &op_id,
            INSTALL_STEP_VERIFY_STAGED,
            OperationStepKind::VerifyStaged,
            serde_json::json!({
                "mod_folder_name": plan.mod_folder_name,
                "component_count": plan.component_manifests.len(),
            }),
        )?;
        let _ = acquisition;

        let mut detected_components = Vec::new();
        if plan.component_manifests.is_empty() {
            detected_components.push(PackageComponentPreviewDto {
                unique_id: plan.manifest.unique_id.as_str().to_string(),
                name: plan.manifest.name.clone(),
                author: plan.manifest.author.clone(),
                version: plan.manifest.version.clone(),
                description: plan.manifest.description.clone(),
                relative_root: plan.mod_folder_name.clone(),
            });
        } else {
            for comp in &plan.component_manifests {
                detected_components.push(PackageComponentPreviewDto {
                    unique_id: comp.manifest.unique_id.as_str().to_string(),
                    name: comp.manifest.name.clone(),
                    author: comp.manifest.author.clone(),
                    version: comp.manifest.version.clone(),
                    description: comp.manifest.description.clone(),
                    relative_root: comp.relative_subfolder.clone(),
                });
            }
        }

        let mut warnings = Vec::new();
        let mut blockers = Vec::new();

        if !plan.dependency_report.smapi_compatible {
            blockers.push("Incompatible with current or missing SMAPI installation".to_string());
        }
        if plan.dependency_report.duplicate_id {
            blockers
                .push("A mod with this UniqueID is already installed in this profile".to_string());
        }
        for finding in &plan.dependency_report.findings {
            if !finding.satisfied {
                if finding.is_required {
                    blockers.push(finding.reason.clone());
                } else {
                    warnings.push(finding.reason.clone());
                }
            }
        }

        Ok(OperationPreviewDto {
            operation_id: op_id.to_string(),
            artifact_hash: artifact.hash.as_str().to_string(),
            original_filename: acquisition.original_filename,
            byte_size: artifact.byte_size,
            detected_components,
            dependencies_satisfied: plan.dependency_report.is_installable,
            warnings,
            blockers,
            affected_profile_component_ids: Vec::new(),
            expected_profile_revision: Some(profile.revision),
        })
    }

    pub fn prepare_removal(
        &self,
        profile_component_id: &ProfileComponentId,
    ) -> AppResult<OperationPreviewDto> {
        let comp = self
            .deployment_repo
            .get_profile_component(profile_component_id)?
            .ok_or_else(|| {
                AppError::validation("COMPONENT_NOT_FOUND", "Profile component not found")
            })?;

        let profile = self
            .profile_repo
            .get_profile(&comp.profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        if profile.state != manager_core::profile::ProfileState::Active {
            return Err(AppError::validation(
                "PROFILE_NOT_ACTIVE",
                "Only active profiles can remove deployments",
            ));
        }
        ensure_profile_write_available(&*self.operation_repo, &comp.profile_id, None)?;

        let deployment = self
            .deployment_repo
            .get_deployment(&comp.deployment_id)?
            .ok_or_else(|| AppError::validation("DEPLOYMENT_NOT_FOUND", "Deployment not found"))?;

        // Find all companion components participating in this deployment
        let all_profile_comps = self
            .deployment_repo
            .list_profile_components(&comp.profile_id)?;
        let affected_components: Vec<_> = all_profile_comps
            .into_iter()
            .filter(|c| c.deployment_id == comp.deployment_id)
            .collect();

        let affected_ids: Vec<String> = affected_components
            .iter()
            .map(|c| c.id.to_string())
            .collect();

        // Check reverse dependencies using canonical DependencyGraph
        let mut manifests = Vec::new();
        let mut target_unique_ids = HashSet::new();

        for pc in self
            .deployment_repo
            .list_profile_components(&comp.profile_id)?
        {
            if pc.enabled {
                if let Some(pkg_comp) = self
                    .package_repo
                    .get_package_component(&pc.package_component_id)?
                {
                    if affected_components.iter().any(|ac| ac.id == pc.id) {
                        target_unique_ids.insert(pkg_comp.unique_id.clone());
                    }
                    manifests.push(pkg_comp.manifest);
                }
            }
        }

        let graph = build_dependency_graph(&manifests, None);
        let mut warnings = Vec::new();
        for target_id in &target_unique_ids {
            let rev_deps = graph.reverse_dependents(target_id);
            for rd in rev_deps {
                if !target_unique_ids.contains(&rd) {
                    warnings.push(format!(
                        "Mod '{}' depends on '{}' and may stop functioning if it is removed.",
                        rd, target_id
                    ));
                }
            }
        }

        let op_id = OperationId::new();
        let removal_plan = serde_json::json!({
            "profile_id": comp.profile_id.to_string(),
            "deployment_id": deployment.id.to_string(),
            "deployment_rel_path": deployment.root_relative_path,
            "removed_profile_component_ids": affected_ids,
        });

        let op = Operation {
            id: op_id,
            kind: OperationKind::ModRemove,
            state: OperationState::Draft,
            game_installation_id: Some(profile.game_installation_id),
            profile_id: Some(comp.profile_id),
            expected_profile_revision: Some(profile.revision),
            plan_schema_version: OPERATION_PLAN_SCHEMA_V2,
            plan_json: removal_plan.to_string(),
            progress_current: Some(0),
            progress_total: Some(2),
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        };
        self.operation_repo.create_operation(&op)?;

        self.operation_repo
            .save_operation_resource(&OperationResource {
                operation_id: op_id,
                resource_kind: ResourceKind::Profile,
                resource_id: comp.profile_id.to_string(),
                access_mode: AccessMode::Write,
            })?;

        Ok(OperationPreviewDto {
            operation_id: op_id.to_string(),
            artifact_hash: deployment.artifact_hash.as_str().to_string(),
            original_filename: deployment.root_relative_path.clone(),
            byte_size: 0,
            detected_components: Vec::new(),
            dependencies_satisfied: true,
            warnings,
            blockers: Vec::new(),
            affected_profile_component_ids: affected_ids,
            expected_profile_revision: Some(profile.revision),
        })
    }
}
