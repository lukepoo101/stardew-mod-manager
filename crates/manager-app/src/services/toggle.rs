//! Enabling and disabling the mods of a profile.
//!
//! A deployment is the unit that moves: every component that arrived in the
//! same package shares one folder, so they are enabled and disabled together.
//! The move and the database update are not one atomic step, so the operation
//! is written to be repeatable: it always reconciles towards the requested
//! state from wherever the files actually are, which means running it again
//! after an interruption heals a half-applied change instead of failing.

use crate::api::dto::{BulkToggleFailureDto, BulkToggleResultDto, ToggleImpactDto};
use crate::error::{AppError, AppResult};
use crate::ports::deployment::DeploymentPort;
use crate::ports::launcher::GameLauncherPort;
use crate::ports::repositories::{
    DeploymentRepository, OperationRepository, PackageCatalogRepository, ProfileRepository,
};
use crate::services::resources::{ensure_resources_available, ResourceClaim, ResourceCoordinator};
use manager_core::deployment::ProfileComponent;
use manager_core::ids::{ProfileComponentId, ProfileId};
use manager_core::operation::ResourceKind;
use manager_core::ports::InstanceLock;
use std::collections::HashSet;
use std::sync::Arc;

pub struct ToggleService {
    resources: Arc<ResourceCoordinator>,
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    deployment: Arc<dyn DeploymentPort>,
    launcher: Arc<dyn GameLauncherPort>,
    instance_lock: Arc<dyn InstanceLock>,
}

struct Group {
    profile_id: ProfileId,
    /// Every profile component that shares the deployment folder.
    members: Vec<ProfileComponent>,
    deployment_rel_path: String,
}

impl ToggleService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resources: Arc<ResourceCoordinator>,
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        operation_repo: Arc<dyn OperationRepository>,
        deployment: Arc<dyn DeploymentPort>,
        launcher: Arc<dyn GameLauncherPort>,
        instance_lock: Arc<dyn InstanceLock>,
    ) -> Self {
        Self {
            resources,
            profile_repo,
            deployment_repo,
            package_repo,
            operation_repo,
            deployment,
            launcher,
            instance_lock,
        }
    }

    /// The distinct deployment groups behind several mods, which must all be
    /// in one profile.
    fn groups(&self, ids: &[ProfileComponentId]) -> AppResult<Vec<Group>> {
        if ids.is_empty() {
            return Err(AppError::validation(
                "NO_MODS_SELECTED",
                "Choose at least one mod",
            ));
        }
        let mut groups: Vec<Group> = Vec::new();
        for id in ids {
            let group = self.group(id)?;
            if groups
                .first()
                .is_some_and(|first| first.profile_id != group.profile_id)
            {
                return Err(AppError::validation(
                    "MODS_IN_DIFFERENT_PROFILES",
                    "The selected mods are not all in the same profile",
                ));
            }
            if !groups
                .iter()
                .any(|known| known.deployment_rel_path == group.deployment_rel_path)
            {
                groups.push(group);
            }
        }
        Ok(groups)
    }

    fn group(&self, id: &ProfileComponentId) -> AppResult<Group> {
        let component = self
            .deployment_repo
            .get_profile_component(id)?
            .ok_or_else(|| {
                AppError::validation("COMPONENT_NOT_FOUND", "That mod is not in any profile")
            })?;
        let deployment = self
            .deployment_repo
            .get_deployment(&component.deployment_id)?
            .ok_or_else(|| {
                AppError::validation("DEPLOYMENT_NOT_FOUND", "The mod's files are not recorded")
            })?;
        let members = self
            .deployment_repo
            .list_profile_components(&component.profile_id)?
            .into_iter()
            .filter(|member| member.deployment_id == component.deployment_id)
            .collect();
        Ok(Group {
            profile_id: component.profile_id,
            members,
            deployment_rel_path: deployment.root_relative_path,
        })
    }

    /// Names the other mods that would be affected, so the user can decide
    /// before anything moves. Nothing here changes state.
    pub fn impact(&self, id: &ProfileComponentId, enable: bool) -> AppResult<ToggleImpactDto> {
        self.impact_many(std::slice::from_ref(id), enable)
    }

    /// The combined effect of enabling or disabling several mods together:
    /// mods sharing a package with any of them move too, requirements met by
    /// another mod in the set are not reported, and only mods outside the set
    /// count as dependents.
    pub fn impact_many(
        &self,
        ids: &[ProfileComponentId],
        enable: bool,
    ) -> AppResult<ToggleImpactDto> {
        let groups = self.groups(ids)?;
        let profile_id = groups[0].profile_id;
        let member_ids: HashSet<_> = groups
            .iter()
            .flat_map(|group| group.members.iter().map(|m| m.id))
            .collect();

        let mut group_mods = Vec::new();
        let mut group_unique_ids = HashSet::new();
        let mut group_requires: Vec<(String, String)> = Vec::new();
        let mut others: Vec<(String, String, Vec<String>)> = Vec::new();
        let mut enabled_other_ids = HashSet::new();

        for pc in self.deployment_repo.list_profile_components(&profile_id)? {
            let Some(component) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            let unique_id = component.unique_id.to_string();
            let mut required: Vec<String> = component
                .manifest
                .dependencies
                .iter()
                .filter(|d| d.is_required)
                .map(|d| d.unique_id.to_string())
                .collect();
            if let Some(host) = &component.manifest.content_pack_for {
                required.push(host.unique_id.to_string());
            }
            if member_ids.contains(&pc.id) {
                group_mods.push(component.name.clone());
                group_unique_ids.insert(unique_id.clone());
                for dependency in required {
                    group_requires.push((component.name.clone(), dependency));
                }
            } else if pc.enabled {
                enabled_other_ids.insert(unique_id.clone());
                others.push((component.name.clone(), unique_id, required));
            }
        }

        let mut dependents = Vec::new();
        let mut missing = Vec::new();
        if enable {
            for (name, dependency) in &group_requires {
                if !group_unique_ids.contains(dependency) && !enabled_other_ids.contains(dependency)
                {
                    missing.push(format!("{name} needs {dependency}, which is not enabled"));
                }
            }
        } else {
            for (name, _, required) in &others {
                if required.iter().any(|dep| group_unique_ids.contains(dep)) {
                    dependents.push(name.clone());
                }
            }
        }
        dependents.sort();
        dependents.dedup();
        missing.sort();
        missing.dedup();

        Ok(ToggleImpactDto {
            affected_mods: group_mods,
            dependents,
            unmet_requirements: missing,
        })
    }

    /// Moves the mod's folder and records the new state, repeatably.
    pub fn set_enabled(&self, id: &ProfileComponentId, enable: bool) -> AppResult<()> {
        let group = self.group(id)?;
        let _locks = self.lock_profile(&group.profile_id)?;
        self.apply(&group, enable)
    }

    /// Enables or disables several mods under one lock. Each package folder is
    /// moved once; a failure on one does not undo the others (every move is
    /// repeatable, so running the same request again finishes the job), and
    /// the result says what happened to each.
    pub fn set_many_enabled(
        &self,
        ids: &[ProfileComponentId],
        enable: bool,
    ) -> AppResult<BulkToggleResultDto> {
        let groups = self.groups(ids)?;
        let _locks = self.lock_profile(&groups[0].profile_id)?;
        let profile_id = groups[0].profile_id;
        // The whole request is written down before any folder moves, so an
        // interruption is finished by recovery rather than left half done.
        let operation_id = manager_core::ids::OperationId::new();
        let now = chrono::Utc::now();
        self.operation_repo
            .create_operation(&manager_core::operation::Operation {
                id: operation_id,
                kind: manager_core::operation::OperationKind::ModToggle,
                state: manager_core::operation::OperationState::Committing,
                game_installation_id: None,
                profile_id: Some(profile_id),
                expected_profile_revision: None,
                plan_schema_version: manager_core::operation::OPERATION_PLAN_SCHEMA_V2,
                plan_json: serde_json::json!({
                    "enable": enable,
                    "targets": groups
                        .iter()
                        .map(|g| serde_json::json!({
                            "folder": g.deployment_rel_path,
                            "components": g.members.iter().map(|m| m.id.to_string()).collect::<Vec<_>>(),
                        }))
                        .collect::<Vec<_>>(),
                })
                .to_string(),
                progress_current: Some(0),
                progress_total: Some(groups.len() as u32),
                error_code: None,
                error_json: None,
                cancellation_requested: false,
                created_at: now,
                updated_at: now,
                completed_at: None,
            })?;
        self.operation_repo.save_operation_resource(
            &manager_core::operation::OperationResource {
                operation_id,
                resource_kind: ResourceKind::Profile,
                resource_id: profile_id.to_string(),
                access_mode: manager_core::operation::AccessMode::Write,
            },
        )?;
        let mut result = BulkToggleResultDto {
            changed: Vec::new(),
            failed: Vec::new(),
        };
        for group in &groups {
            let names = self.member_names(group);
            match self.apply(group, enable) {
                Ok(()) => result.changed.extend(names),
                Err(error) => {
                    result
                        .failed
                        .extend(names.into_iter().map(|name| BulkToggleFailureDto {
                            name,
                            message: error.summary.clone(),
                        }))
                }
            }
        }
        result.changed.sort();
        result.failed.sort_by(|a, b| a.name.cmp(&b.name));
        let (state, code, message) = if result.failed.is_empty() {
            (
                manager_core::operation::OperationState::Succeeded,
                None,
                None,
            )
        } else {
            (
                manager_core::operation::OperationState::Failed,
                Some("SOME_MODS_NOT_CHANGED"),
                Some(format!(
                    "{} changed; not changed: {}",
                    result.changed.len(),
                    result
                        .failed
                        .iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            )
        };
        crate::services::operation_lifecycle::OperationLifecycle::new(self.operation_repo.clone())
            .transition(&operation_id, state, code, message)?;
        Ok(result)
    }

    fn member_names(&self, group: &Group) -> Vec<String> {
        group
            .members
            .iter()
            .map(|member| {
                self.package_repo
                    .get_package_component(&member.package_component_id)
                    .ok()
                    .flatten()
                    .map(|component| component.name)
                    .unwrap_or_else(|| member.id.to_string())
            })
            .collect()
    }

    /// Holds the profile's write claim and the instance lock, and refuses while
    /// the game is running.
    fn lock_profile(
        &self,
        profile_id: &ProfileId,
    ) -> AppResult<(
        crate::services::ResourceLease,
        Box<dyn std::any::Any + Send + Sync>,
    )> {
        let claims = vec![ResourceClaim::write(
            ResourceKind::Profile,
            profile_id.to_string(),
        )];
        ensure_resources_available(&*self.operation_repo, &claims, None)?;
        let lease = self.resources.try_acquire(&claims)?;
        let guard = self
            .instance_lock
            .acquire_guard()
            .map_err(AppError::instance_locked)?;
        if self.launcher.is_game_running(None) {
            return Err(AppError::game_running(
                "Stop Stardew Valley before enabling or disabling mods",
            ));
        }
        Ok((lease, guard))
    }

    /// SMAPI loads only one mod per UniqueID, so enabling a copy while
    /// another copy is enabled would leave which one runs to chance.
    fn refuse_live_duplicates(&self, group: &Group) -> AppResult<()> {
        let members: HashSet<_> = group.members.iter().map(|m| m.id).collect();
        let mut wanted = std::collections::HashMap::new();
        for member in &group.members {
            if let Some(component) = self
                .package_repo
                .get_package_component(&member.package_component_id)?
            {
                wanted.insert(component.unique_id.as_str().to_lowercase(), component.name);
            }
        }
        for pc in self
            .deployment_repo
            .list_profile_components(&group.profile_id)?
        {
            if !pc.enabled || members.contains(&pc.id) {
                continue;
            }
            let Some(other) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            if let Some(name) = wanted.get(&other.unique_id.as_str().to_lowercase()) {
                return Err(AppError::validation(
                    "DUPLICATE_UNIQUE_ID",
                    format!(
                        "'{name}' has the same ID as '{}' {}, which is already enabled. SMAPI would load only one of them; disable that copy first.",
                        other.name, other.version
                    ),
                ));
            }
        }
        Ok(())
    }

    fn apply(&self, group: &Group, enable: bool) -> AppResult<()> {
        let mut profile = self
            .profile_repo
            .get_profile(&group.profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        if profile.state != manager_core::profile::ProfileState::Active {
            return Err(AppError::validation(
                "PROFILE_NOT_ACTIVE",
                "Only active profiles can be changed",
            ));
        }

        if enable {
            self.refuse_live_duplicates(group)?;
        }

        // Reconcile the files towards the request from wherever they really are.
        let in_mods = self
            .deployment
            .deployment_exists(&group.profile_id, &group.deployment_rel_path)?;
        let moved = if enable && !in_mods {
            self.deployment
                .enable_deployment(&group.profile_id, &group.deployment_rel_path)?;
            true
        } else if !enable && in_mods {
            self.deployment
                .disable_deployment(&group.profile_id, &group.deployment_rel_path)?;
            true
        } else {
            false
        };

        let mut changed = false;
        let result = (|| -> AppResult<()> {
            for member in &group.members {
                if member.enabled != enable {
                    let mut updated = member.clone();
                    updated.enabled = enable;
                    self.deployment_repo.save_profile_component(&updated)?;
                    changed = true;
                }
            }
            if changed || moved {
                profile.bump_revision();
                self.profile_repo.save_profile(&profile)?;
            }
            Ok(())
        })();

        if let Err(error) = result {
            // Put the files back so they keep matching the recorded state.
            if moved {
                let _ = if enable {
                    self.deployment
                        .disable_deployment(&group.profile_id, &group.deployment_rel_path)
                } else {
                    self.deployment
                        .enable_deployment(&group.profile_id, &group.deployment_rel_path)
                };
            }
            return Err(error);
        }
        Ok(())
    }
}
