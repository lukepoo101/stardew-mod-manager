use crate::api::dto::{ModRequirementDto, OperationPreviewDto, PackageComponentPreviewDto};
use crate::error::{AppError, AppResult};
use crate::ports::deployment::{ArchiveInspectorPort, StagedContentVerifierPort, StagingPort};
use crate::ports::repositories::{
    DeploymentRepository, OperationRepository, PackageCatalogRepository, ProfileRepository,
    SmapiRepository,
};
use crate::services::operation_lifecycle::OperationLifecycle;
use crate::services::packages::PackagesService;
use crate::services::resources::ensure_profile_write_available_with;
use chrono::Utc;
use manager_core::dependency::relations::{EdgeKind, EdgeStatus};
use manager_core::dependency::{check_requirements, AvailableMod, DependencyReport};
use manager_core::ids::ModUniqueId;
use manager_core::ids::{OperationId, ProfileComponentId, ProfileId};
use manager_core::operation::{
    AccessMode, Operation, OperationKind, OperationResource, OperationState, OperationStepKind,
    ResourceKind, INSTALL_STEP_INSPECT_AND_STAGE, INSTALL_STEP_RETAIN_ARTIFACT,
    INSTALL_STEP_VERIFY_STAGED, OPERATION_PLAN_SCHEMA_V2,
};
use manager_core::package::PackageComponent;
use manager_core::version::SmapiVersion;

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
    freeze: Option<Arc<crate::services::ProfileFreeze>>,
    liveness: Option<Arc<crate::services::resources::OperationLiveness>>,
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
            freeze: None,
            liveness: None,
        }
    }

    /// Refuses installs and removals in frozen profiles.
    pub fn with_freeze(mut self, freeze: Arc<crate::services::ProfileFreeze>) -> Self {
        self.freeze = Some(freeze);
        self
    }

    /// Tells a change in progress from an interrupted one when a new review
    /// is refused.
    pub fn with_liveness(
        mut self,
        liveness: Arc<crate::services::resources::OperationLiveness>,
    ) -> Self {
        self.liveness = Some(liveness);
        self
    }

    fn ensure_not_frozen(&self, profile_id: &ProfileId) -> AppResult<()> {
        match &self.freeze {
            Some(freeze) => freeze.ensure_not_frozen(profile_id),
            None => Ok(()),
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
        self.ensure_not_frozen(profile_id)?;
        ensure_profile_write_available_with(
            &*self.operation_repo,
            self.liveness.as_deref(),
            profile_id,
            None,
        )?;

        // 1. Authoritative retention of source bytes
        let (artifact, acquisition) = self.packages.retain_local_package(source_zip)?;
        let artifact_path = self.packages.get_artifact_path(&artifact.hash)?;

        let op_id = OperationId::new();
        let staging_dir = self.staging.create_staging_dir(profile_id, &op_id)?;

        // Collect existing installed manifests in profile
        let profile_comps = self.deployment_repo.list_profile_components(profile_id)?;
        let mut installed = Vec::new();
        for pc in &profile_comps {
            if let Some(comp) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            {
                installed.push((pc.enabled, comp));
            }
        }
        let installed_manifests: Vec<_> = installed
            .iter()
            .map(|(_, comp)| (comp.unique_id.clone(), comp.version.clone()))
            .collect();

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

        // The package's own mods, each with the folder it comes from.
        let package: Vec<(&manager_core::Manifest, &str)> = if plan.component_manifests.is_empty() {
            vec![(&plan.manifest, plan.mod_folder_name.as_str())]
        } else {
            plan.component_manifests
                .iter()
                .map(|comp| (&comp.manifest, comp.relative_subfolder.as_str()))
                .collect()
        };
        let InstallReview {
            components: detected_components,
            blockers,
            other_blockers,
            mut warnings,
        } = review_package(
            &package,
            &installed,
            &plan.dependency_report,
            smapi_version.as_deref(),
        );
        let incoming_ids: Vec<&str> = package.iter().map(|(m, _)| m.unique_id.as_str()).collect();
        let mut replaces = self.replacements(profile_id, &detected_components)?;
        for replacement in &mut replaces {
            replacement.stops_loading = stops_loading(replacement, &incoming_ids, &installed);
        }
        if !plan.not_installed.is_empty() {
            const SHOWN: usize = 5;
            let mut names: Vec<&str> = plan
                .not_installed
                .iter()
                .take(SHOWN)
                .map(String::as_str)
                .collect();
            let more = plan.not_installed.len().saturating_sub(SHOWN);
            let more = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            names.sort();
            warnings.push(format!(
                "{} file(s) in the archive are outside the mod's folder and will not be installed: {}{more}. They stay in the stored archive.",
                plan.not_installed.len(),
                names.join(", ")
            ));
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
            other_blockers,
            affected_profile_component_ids: Vec::new(),
            expected_profile_revision: Some(profile.revision),
            replaces,
            files: plan
                .trusted_inventory
                .iter()
                .filter(|e| e.entry_type == manager_core::install::InventoryEntryType::File)
                .map(|e| crate::api::dto::PlanFileDto {
                    path: e.relative_path.clone(),
                    size_bytes: e.size_bytes,
                    installed: true,
                })
                .chain(plan.not_installed.iter().map(|path| {
                    crate::api::dto::PlanFileDto {
                        path: path.clone(),
                        size_bytes: plan
                            .not_installed_sizes
                            .iter()
                            .find(|(p, _)| p == path)
                            .map(|(_, size)| *size)
                            .unwrap_or(0),
                        installed: false,
                    }
                }))
                .collect(),
        })
    }

    /// Installed mods with the same UniqueIDs as the incoming components.
    fn replacements(
        &self,
        profile_id: &ProfileId,
        incoming: &[PackageComponentPreviewDto],
    ) -> AppResult<Vec<crate::api::dto::ReplacementDto>> {
        let mut out = Vec::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            let Some(installed) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            let Some(new) = incoming.iter().find(|c| {
                c.unique_id
                    .eq_ignore_ascii_case(installed.unique_id.as_str())
            }) else {
                continue;
            };
            let direction = match (
                manager_core::version::SmapiVersion::parse(&installed.version),
                manager_core::version::SmapiVersion::parse(&new.version),
            ) {
                (Ok(old), Ok(next)) if next > old => "upgrade",
                (Ok(old), Ok(next)) if next < old => "downgrade",
                (Ok(_), Ok(_)) => "same",
                _ if installed.version == new.version => "same",
                // Unparseable versions: say it changes, not which way.
                _ => "different",
            };
            out.push(crate::api::dto::ReplacementDto {
                profile_component_id: pc.id.to_string(),
                unique_id: installed.unique_id.to_string(),
                name: installed.name,
                installed_version: installed.version,
                incoming_version: new.version.clone(),
                direction: direction.to_string(),
                stops_loading: Vec::new(),
            });
        }
        Ok(out)
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
        self.ensure_not_frozen(&comp.profile_id)?;
        ensure_profile_write_available_with(
            &*self.operation_repo,
            self.liveness.as_deref(),
            &comp.profile_id,
            None,
        )?;

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

        // A disabled mod's files live outside the mods folder, where removal
        // does not look. Enabling it first keeps removal on one well-tested path.
        if affected_components.iter().any(|c| !c.enabled) {
            return Err(AppError::validation(
                "MOD_DISABLED",
                "This mod is disabled. Enable it first, then remove it.",
            ));
        }

        let affected_ids: Vec<String> = affected_components
            .iter()
            .map(|c| c.id.to_string())
            .collect();

        // Check reverse dependencies using canonical DependencyGraph
        let mut manifests = Vec::new();
        let mut target_unique_ids = HashSet::new();
        // The removed mods by name, so the review need not show a UniqueID
        // or a package checksum.
        let mut removed_components = Vec::new();

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
                        removed_components.push(PackageComponentPreviewDto {
                            unique_id: pkg_comp.unique_id.as_str().to_string(),
                            name: pkg_comp.name.clone(),
                            author: pkg_comp.author.clone(),
                            version: pkg_comp.version.clone(),
                            description: pkg_comp.description.clone(),
                            relative_root: pkg_comp.relative_component_root.clone(),
                            requirements: Vec::new(),
                        });
                    }
                    manifests.push(pkg_comp.manifest);
                }
            }
        }

        // Required dependents (and content packs for a host) stop loading;
        // optional ones only lose what they did with it.
        let removed = |id: &ModUniqueId| {
            target_unique_ids
                .iter()
                .any(|t| t.as_str().eq_ignore_ascii_case(id.as_str()))
        };
        let mut warnings = Vec::new();
        for manifest in &manifests {
            if removed(&manifest.unique_id) {
                continue;
            }
            if let Some(host) = &manifest.content_pack_for {
                if removed(&host.unique_id) {
                    warnings.push(format!(
                        "'{}' is a content pack for '{}' and will not load without it.",
                        manifest.name, host.unique_id
                    ));
                }
            }
            for dependency in &manifest.dependencies {
                if !removed(&dependency.unique_id) {
                    continue;
                }
                warnings.push(if dependency.is_required {
                    format!(
                        "'{}' requires '{}' and will not load without it.",
                        manifest.name, dependency.unique_id
                    )
                } else {
                    format!(
                        "'{}' can use '{}' (optional); it still loads, without that part.",
                        manifest.name, dependency.unique_id
                    )
                });
            }
        }
        // Mods that stop loading because something they need stops loading
        // too, followed until nothing else changes. Optional links never
        // carry this on.
        let mut gone: Vec<String> = target_unique_ids
            .iter()
            .map(|id| id.as_str().to_lowercase())
            .collect();
        let mut stops: Vec<String> = Vec::new();
        for manifest in &manifests {
            let needs_removed = manifest
                .content_pack_for
                .iter()
                .any(|h| removed(&h.unique_id))
                || manifest
                    .dependencies
                    .iter()
                    .any(|d| d.is_required && removed(&d.unique_id));
            if needs_removed && !removed(&manifest.unique_id) {
                gone.push(manifest.unique_id.as_str().to_lowercase());
                stops.push(manifest.unique_id.as_str().to_lowercase());
            }
        }
        loop {
            let mut added = false;
            for manifest in &manifests {
                let id = manifest.unique_id.as_str().to_lowercase();
                if gone.contains(&id) {
                    continue;
                }
                let via = manifest
                    .content_pack_for
                    .iter()
                    .map(|h| h.unique_id.as_str().to_lowercase())
                    .chain(
                        manifest
                            .dependencies
                            .iter()
                            .filter(|d| d.is_required)
                            .map(|d| d.unique_id.as_str().to_lowercase()),
                    )
                    .find(|needed| stops.contains(needed));
                if let Some(via) = via {
                    let via_name = manifests
                        .iter()
                        .find(|m| m.unique_id.as_str().eq_ignore_ascii_case(&via))
                        .map(|m| m.name.clone())
                        .unwrap_or(via);
                    warnings.push(format!(
                        "'{}' needs '{}', which stops loading too, so it will not load either.",
                        manifest.name, via_name
                    ));
                    gone.push(id.clone());
                    stops.push(id);
                    added = true;
                }
            }
            if !added {
                break;
            }
        }
        warnings.sort();
        warnings.dedup();

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

        // The archive the mods came from, as the user knew it.
        let original_filename = self
            .package_repo
            .get_acquisitions_for_artifact(&deployment.artifact_hash)?
            .into_iter()
            .min_by_key(|a| a.acquired_at)
            .map(|a| a.original_filename)
            .unwrap_or_else(|| deployment.root_relative_path.clone());

        Ok(OperationPreviewDto {
            operation_id: op_id.to_string(),
            artifact_hash: deployment.artifact_hash.as_str().to_string(),
            original_filename,
            byte_size: 0,
            detected_components: removed_components,
            dependencies_satisfied: true,
            warnings,
            blockers: Vec::new(),
            other_blockers: Vec::new(),
            affected_profile_component_ids: affected_ids,
            replaces: Vec::new(),
            expected_profile_revision: Some(profile.revision),
            files: Vec::new(),
        })
    }
}

/// What an install review says about a package before anything changes.
struct InstallReview {
    components: Vec<PackageComponentPreviewDto>,
    /// Everything that stops the install, requirement problems included.
    blockers: Vec<String>,
    /// The blockers no other archive could resolve.
    other_blockers: Vec<String>,
    warnings: Vec<String>,
}

fn kind_name(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Required => "required",
        EdgeKind::Optional => "optional",
        EdgeKind::ContentPackFor => "content_pack_for",
    }
}

fn status_name(status: EdgeStatus) -> &'static str {
    match status {
        EdgeStatus::Satisfied => "satisfied",
        EdgeStatus::Missing => "missing",
        EdgeStatus::Disabled => "disabled",
        EdgeStatus::TooOld => "too_old",
    }
}

fn push_once(list: &mut Vec<String>, line: String) {
    if !list.contains(&line) {
        list.push(line);
    }
}

/// Checks a package's mods against the profile and each other, and says in
/// plain words what blocks it, naming the mod that needs each thing.
///
/// `report` is the archive inspector's verdict, which the commit enforces; the
/// review only explains it, so anything the report blocks without a reason
/// found here still gets one.
fn review_package(
    package: &[(&manager_core::Manifest, &str)],
    installed: &[(bool, PackageComponent)],
    report: &DependencyReport,
    smapi_version: Option<&str>,
) -> InstallReview {
    // The package's own mods first: they are what the others will load with.
    let available: Vec<AvailableMod> = package
        .iter()
        .map(|(manifest, _)| AvailableMod {
            unique_id: manifest.unique_id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            enabled: true,
        })
        .chain(installed.iter().map(|(enabled, comp)| AvailableMod {
            unique_id: comp.unique_id.clone(),
            name: comp.name.clone(),
            version: comp.version.clone(),
            enabled: *enabled,
        }))
        .collect();

    let mut other_blockers = Vec::new();
    if !report.smapi_compatible {
        let have = smapi_version.unwrap_or(manager_core::smapi::PINNED_SMAPI_VERSION);
        for (manifest, _) in package {
            let Some(minimum) = manifest.minimum_api_version.as_deref() else {
                continue;
            };
            if let (Ok(need), Ok(current)) =
                (SmapiVersion::parse(minimum), SmapiVersion::parse(have))
            {
                if current < need {
                    push_once(
                        &mut other_blockers,
                        match smapi_version {
                            Some(current) => format!(
                                "{} needs SMAPI {minimum} or newer; this game has SMAPI {current}. Update SMAPI first, then install it.",
                                manifest.name
                            ),
                            None => format!(
                                "{} needs SMAPI {minimum} or newer, which is newer than the SMAPI {have} this manager installs.",
                                manifest.name
                            ),
                        },
                    );
                }
            }
        }
        if other_blockers.is_empty() {
            other_blockers.push("This mod needs a newer SMAPI than this game has.".to_string());
        }
    }
    for id in &report.duplicate_in_package {
        let folders: Vec<String> = package
            .iter()
            .filter(|(manifest, _)| {
                manifest
                    .unique_id
                    .as_str()
                    .eq_ignore_ascii_case(id.as_str())
            })
            .map(|(_, folder)| {
                if folder.is_empty() {
                    "the archive's top level".to_string()
                } else {
                    format!("'{folder}'")
                }
            })
            .collect();
        other_blockers.push(format!(
            "This archive has {} mods with the UniqueID '{}' (in {}). SMAPI loads only one mod per UniqueID, so the archive cannot be installed as it is. Check that this is the right download.",
            folders.len(),
            id,
            folders.join(" and ")
        ));
    }
    if report.duplicate_id {
        let names: Vec<&str> = package
            .iter()
            .filter(|(manifest, _)| {
                installed.iter().any(|(_, comp)| {
                    comp.unique_id
                        .as_str()
                        .eq_ignore_ascii_case(manifest.unique_id.as_str())
                })
            })
            .map(|(manifest, _)| manifest.name.as_str())
            .collect();
        // Also set for twins inside the package, which are named above.
        if !names.is_empty() {
            other_blockers.push(format!(
                "A mod with this UniqueID is already installed in this profile: {}.",
                names.join(", ")
            ));
        }
    }

    let mut blockers = other_blockers.clone();
    let mut warnings = Vec::new();
    let mut components = Vec::new();
    for (manifest, folder) in package {
        let checks = check_requirements(manifest, &available);
        for check in &checks {
            let id = check.unique_id.as_str();
            let needed = check.name.as_deref().unwrap_or(id);
            let mod_name = &manifest.name;
            match (check.kind, check.status) {
                (_, EdgeStatus::Satisfied) => {}
                (EdgeKind::Optional, EdgeStatus::Missing) => push_once(
                    &mut warnings,
                    format!("{mod_name} can use {id} (optional), which is not installed. It works without it."),
                ),
                (EdgeKind::Optional, _) => {}
                (EdgeKind::ContentPackFor, EdgeStatus::Missing) => push_once(
                    &mut blockers,
                    format!("{mod_name} is a content pack for {id}, which is not installed. Install {id} first."),
                ),
                (_, EdgeStatus::Missing) => push_once(
                    &mut blockers,
                    format!("{mod_name} needs {id}, which is not installed. Install {id} first."),
                ),
                (_, EdgeStatus::TooOld) => push_once(
                    &mut blockers,
                    format!(
                        "{mod_name} needs {needed} {} or newer; {} is installed. Update {needed} first.",
                        check.minimum_version.as_deref().unwrap_or_default(),
                        check.installed_version.as_deref().unwrap_or("an older version"),
                    ),
                ),
                (_, EdgeStatus::Disabled) => push_once(
                    &mut warnings,
                    format!("{mod_name} needs {needed}, which is installed but disabled. Enable {needed} too, or {mod_name} will not load."),
                ),
            }
        }
        components.push(PackageComponentPreviewDto {
            unique_id: manifest.unique_id.as_str().to_string(),
            name: manifest.name.clone(),
            author: manifest.author.clone(),
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            relative_root: folder.to_string(),
            requirements: checks
                .iter()
                .map(|check| ModRequirementDto {
                    unique_id: check.unique_id.as_str().to_string(),
                    name: check.name.clone(),
                    installed_version: check.installed_version.clone(),
                    minimum_version: check.minimum_version.clone(),
                    kind: kind_name(check.kind).to_string(),
                    status: status_name(check.status).to_string(),
                })
                .collect(),
        });
    }

    if !report.is_installable && blockers.is_empty() {
        for finding in report
            .findings
            .iter()
            .filter(|f| f.is_required && !f.satisfied)
        {
            push_once(&mut blockers, finding.reason.clone());
        }
        if blockers.is_empty() {
            blockers.push("It cannot be installed in this profile as it is.".to_string());
        }
    }

    InstallReview {
        components,
        blockers,
        other_blockers,
        warnings,
    }
}

/// Enabled mods outside the package that need a newer version of a mod
/// than the one coming in, so would stop loading after the replacement.
fn stops_loading(
    replacement: &crate::api::dto::ReplacementDto,
    incoming_ids: &[&str],
    installed: &[(bool, PackageComponent)],
) -> Vec<String> {
    let Ok(incoming) = SmapiVersion::parse(&replacement.incoming_version) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (enabled, comp) in installed {
        // Mods replaced along with it are reviewed through their own
        // requirements instead.
        if !enabled
            || incoming_ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case(comp.unique_id.as_str()))
        {
            continue;
        }
        let manifest = &comp.manifest;
        let needs = manifest
            .content_pack_for
            .iter()
            .map(|host| (&host.unique_id, host.minimum_version.as_deref()))
            .chain(
                manifest
                    .dependencies
                    .iter()
                    .filter(|d| d.is_required)
                    .map(|d| (&d.unique_id, d.minimum_version.as_deref())),
            );
        for (id, minimum) in needs {
            if !id.as_str().eq_ignore_ascii_case(&replacement.unique_id) {
                continue;
            }
            let Some(minimum) = minimum else { continue };
            if SmapiVersion::parse(minimum).is_ok_and(|need| incoming < need) {
                push_once(
                    &mut out,
                    format!(
                        "{} needs {} {minimum} or newer, so it will not load with {}.",
                        comp.name, replacement.name, replacement.incoming_version
                    ),
                );
            }
        }
    }
    out
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use manager_core::ids::{ArtifactHash, PackageComponentId};
    use manager_core::manifest::{ContentPackFor, ModDependency};

    fn manifest(id: &str, name: &str, version: &str) -> manager_core::Manifest {
        manager_core::Manifest {
            unique_id: ModUniqueId::new(id),
            name: name.into(),
            author: "A".into(),
            version: version.into(),
            description: None,
            entry_dll: None,
            minimum_api_version: None,
            minimum_game_version: None,
            update_keys: Vec::new(),
            dependencies: Vec::new(),
            content_pack_for: None,
        }
    }

    fn requires(
        mut m: manager_core::Manifest,
        id: &str,
        min: Option<&str>,
    ) -> manager_core::Manifest {
        m.dependencies.push(ModDependency {
            unique_id: ModUniqueId::new(id),
            minimum_version: min.map(str::to_string),
            is_required: true,
        });
        m
    }

    fn installed(manifest: manager_core::Manifest, enabled: bool) -> (bool, PackageComponent) {
        (
            enabled,
            PackageComponent {
                id: PackageComponentId::new(),
                artifact_hash: ArtifactHash::new("a".repeat(64)),
                unique_id: manifest.unique_id.clone(),
                name: manifest.name.clone(),
                author: manifest.author.clone(),
                version: manifest.version.clone(),
                description: None,
                relative_component_root: String::new(),
                raw_manifest: String::new(),
                manifest,
            },
        )
    }

    fn report(
        package: &[&manager_core::Manifest],
        installed: &[(bool, PackageComponent)],
    ) -> DependencyReport {
        let installed: Vec<_> = installed
            .iter()
            .map(|(_, c)| (c.unique_id.clone(), c.version.clone()))
            .collect();
        let manifests: Vec<_> = package.iter().map(|m| (*m).clone()).collect();
        manager_core::dependency::evaluate_bundle_dependencies(
            &manifests,
            &installed,
            Some("4.1.10"),
        )
    }

    #[test]
    fn a_missing_requirement_names_the_mod_that_needs_it_once() {
        let a = requires(
            manifest("Two.A", "Two A", "1.0.0"),
            "Pat.ContentPatcher",
            None,
        );
        let b = requires(
            manifest("Two.B", "Two B", "1.0.0"),
            "Pat.ContentPatcher",
            None,
        );
        let review = review_package(&[(&a, "A"), (&b, "B")], &[], &report(&[&a, &b], &[]), None);
        assert_eq!(
            review.blockers,
            vec![
                "Two A needs Pat.ContentPatcher, which is not installed. Install Pat.ContentPatcher first.",
                "Two B needs Pat.ContentPatcher, which is not installed. Install Pat.ContentPatcher first.",
            ]
        );
        assert!(review.other_blockers.is_empty());
        let requirement = &review.components[0].requirements[0];
        assert_eq!(
            (requirement.kind.as_str(), requirement.status.as_str()),
            ("required", "missing")
        );
    }

    #[test]
    fn a_disabled_requirement_is_a_warning_and_too_old_is_a_blocker() {
        let fan = requires(manifest("Ver.Fan", "Ver Fan", "1.0.0"), "Ver.Mod", None);
        let profile = [installed(
            manifest("Ver.Mod", "Versioned Mod", "1.0.0"),
            false,
        )];
        let review = review_package(&[(&fan, "Fan")], &profile, &report(&[&fan], &profile), None);
        assert!(review.blockers.is_empty());
        assert_eq!(
            review.warnings,
            vec!["Ver Fan needs Versioned Mod, which is installed but disabled. Enable Versioned Mod too, or Ver Fan will not load."]
        );

        let picky = requires(
            manifest("Picky", "Picky Mod", "1.0.0"),
            "spacechase0.SpaceCore",
            Some("9.0.0"),
        );
        let profile = [installed(
            manifest("spacechase0.SpaceCore", "SpaceCore", "1.25.0"),
            true,
        )];
        let review = review_package(
            &[(&picky, "Picky")],
            &profile,
            &report(&[&picky], &profile),
            None,
        );
        assert_eq!(
            review.blockers,
            vec!["Picky Mod needs SpaceCore 9.0.0 or newer; 1.25.0 is installed. Update SpaceCore first."]
        );
        assert_eq!(review.components[0].requirements[0].status, "too_old");
    }

    #[test]
    fn package_problems_are_named_and_kept_apart_from_requirements() {
        let mut big = manifest("Big.Map", "Big Map", "1.0.0");
        big.minimum_api_version = Some("9.0.0".into());
        let review = review_package(&[(&big, "Big")], &[], &report(&[&big], &[]), Some("4.1.10"));
        assert_eq!(
            review.other_blockers,
            vec!["Big Map needs SMAPI 9.0.0 or newer; this game has SMAPI 4.1.10. Update SMAPI first, then install it."]
        );
        assert_eq!(review.blockers, review.other_blockers);

        let one = manifest("Twin.Mod", "Twin", "1.0.0");
        let two = manifest("Twin.Mod", "Twin", "1.0.0");
        let review = review_package(
            &[(&one, "First"), (&two, "Second")],
            &[],
            &report(&[&one, &two], &[]),
            None,
        );
        assert_eq!(
            review.other_blockers.len(),
            1,
            "{:?}",
            review.other_blockers
        );
        assert!(review.other_blockers[0]
            .contains("2 mods with the UniqueID 'Twin.Mod' (in 'First' and 'Second')"));
        assert!(!review.other_blockers[0].contains("already installed"));
    }

    #[test]
    fn a_content_pack_says_what_it_is_for() {
        let mut pack = manifest("Me.Pack", "My Pack", "1.0.0");
        pack.content_pack_for = Some(ContentPackFor {
            unique_id: ModUniqueId::new("Pathoschild.ContentPatcher"),
            minimum_version: None,
        });
        let review = review_package(&[(&pack, "Pack")], &[], &report(&[&pack], &[]), None);
        assert_eq!(
            review.blockers,
            vec!["My Pack is a content pack for Pathoschild.ContentPatcher, which is not installed. Install Pathoschild.ContentPatcher first."]
        );
        assert_eq!(
            review.components[0].requirements[0].kind,
            "content_pack_for"
        );
    }

    #[test]
    fn a_downgrade_names_the_mods_that_stop_loading() {
        let middle = requires(
            manifest("Chain.Middle", "Chain Middle", "1.0.0"),
            "Chain.Core",
            Some("2.0.0"),
        );
        let off = requires(
            manifest("Chain.Off", "Chain Off", "1.0.0"),
            "Chain.Core",
            Some("2.0.0"),
        );
        let fine = requires(
            manifest("Chain.Fine", "Chain Fine", "1.0.0"),
            "Chain.Core",
            Some("1.0.0"),
        );
        let profile = [
            installed(manifest("Chain.Core", "Chain Core", "3.0.0"), true),
            installed(middle, true),
            installed(off, false),
            installed(fine, true),
        ];
        let replacement = crate::api::dto::ReplacementDto {
            profile_component_id: "c".into(),
            unique_id: "Chain.Core".into(),
            name: "Chain Core".into(),
            installed_version: "3.0.0".into(),
            incoming_version: "1.5.0".into(),
            direction: "downgrade".into(),
            stops_loading: Vec::new(),
        };
        assert_eq!(
            stops_loading(&replacement, &["Chain.Core"], &profile),
            vec!["Chain Middle needs Chain Core 2.0.0 or newer, so it will not load with 1.5.0."]
        );
    }
}
