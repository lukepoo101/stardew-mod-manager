//! Enabling and disabling the mods of a profile.
//!
//! A deployment is the unit that moves: every component that arrived in the
//! same package shares one folder, so they are enabled and disabled together.
//! The move and the database update are not one atomic step, so the operation
//! is written to be repeatable: it always reconciles towards the requested
//! state from wherever the files actually are, which means running it again
//! after an interruption heals a half-applied change instead of failing.

use crate::api::dto::ToggleImpactDto;
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
        let group = self.group(id)?;
        let member_ids: HashSet<_> = group.members.iter().map(|m| m.id).collect();

        let mut group_mods = Vec::new();
        let mut group_unique_ids = HashSet::new();
        let mut group_requires: Vec<(String, String)> = Vec::new();
        let mut others: Vec<(String, String, Vec<String>)> = Vec::new();
        let mut enabled_other_ids = HashSet::new();

        for pc in self
            .deployment_repo
            .list_profile_components(&group.profile_id)?
        {
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
        let claims = vec![ResourceClaim::write(
            ResourceKind::Profile,
            group.profile_id.to_string(),
        )];
        ensure_resources_available(&*self.operation_repo, &claims, None)?;
        let _lease = self.resources.try_acquire(&claims)?;
        let _guard = self
            .instance_lock
            .acquire_guard()
            .map_err(AppError::instance_locked)?;
        if self.launcher.is_game_running(None) {
            return Err(AppError::game_running(
                "Stop Stardew Valley before enabling or disabling mods",
            ));
        }

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
