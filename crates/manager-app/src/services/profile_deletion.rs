//! Deleting an archived profile.
//!
//! Only archived profiles can be deleted, so the active profile is never the
//! target. The profile's own folder is moved to the manager's trash before its
//! records are removed, and moved back if that fails. Packages are never
//! deleted here: other profiles may use them, and storage cleanup handles the
//! ones nothing uses. Operation and session history is kept, and the deletion
//! is recorded as an operation of its own.

use crate::api::dto::ProfileDeletePreviewDto;
use crate::error::{AppError, AppResult};
use crate::ports::launcher::GameLauncherPort;
use crate::ports::profile_folders::ProfileFolderPort;
use crate::ports::repositories::{DeploymentRepository, OperationRepository, ProfileRepository};
use crate::services::resources::{ensure_resources_available, ResourceClaim, ResourceCoordinator};
use chrono::Utc;
use manager_core::ids::{OperationId, ProfileId};
use manager_core::operation::{Operation, OperationKind, OperationState, ResourceKind};
use manager_core::ports::InstanceLock;
use manager_core::profile::{Profile, ProfileState};
use std::collections::HashSet;
use std::sync::Arc;

/// What is kept with a deleted profile's folder so it can be brought back:
/// its name and each mod's exact package, enabled state and folder.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeletionRecord {
    pub profile_id: String,
    pub name: String,
    pub description: Option<String>,
    pub game_installation_id: String,
    pub deleted_at: String,
    pub mods: Vec<DeletedMod>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeletedMod {
    pub unique_id: String,
    pub name: String,
    pub version: String,
    pub artifact_hash: String,
    pub enabled: bool,
    /// The mod's folder inside the profile, for its settings.
    pub folder: String,
}

pub struct ProfileDeletionService {
    resources: Arc<ResourceCoordinator>,
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    folders: Arc<dyn ProfileFolderPort>,
    launcher: Arc<dyn GameLauncherPort>,
    instance_lock: Arc<dyn InstanceLock>,
    package_repo: Option<Arc<dyn crate::ports::repositories::PackageCatalogRepository>>,
}

impl ProfileDeletionService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resources: Arc<ResourceCoordinator>,
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        operation_repo: Arc<dyn OperationRepository>,
        folders: Arc<dyn ProfileFolderPort>,
        launcher: Arc<dyn GameLauncherPort>,
        instance_lock: Arc<dyn InstanceLock>,
    ) -> Self {
        Self {
            resources,
            profile_repo,
            deployment_repo,
            operation_repo,
            folders,
            launcher,
            instance_lock,
            package_repo: None,
        }
    }

    /// Keeps a record of each deleted profile's mods with its folder in the
    /// trash, so the profile can be brought back from the stored archives.
    pub fn with_records(
        mut self,
        package_repo: Arc<dyn crate::ports::repositories::PackageCatalogRepository>,
    ) -> Self {
        self.package_repo = Some(package_repo);
        self
    }

    fn record(&self, profile: &Profile) -> AppResult<Option<DeletionRecord>> {
        let Some(package_repo) = &self.package_repo else {
            return Ok(None);
        };
        let mut mods = Vec::new();
        for pc in self.deployment_repo.list_profile_components(&profile.id)? {
            let (Some(component), Some(deployment)) = (
                package_repo.get_package_component(&pc.package_component_id)?,
                self.deployment_repo.get_deployment(&pc.deployment_id)?,
            ) else {
                continue;
            };
            mods.push(DeletedMod {
                unique_id: component.unique_id.to_string(),
                name: component.name,
                version: component.version,
                artifact_hash: deployment.artifact_hash.to_string(),
                enabled: pc.enabled,
                folder: deployment.root_relative_path,
            });
        }
        Ok(Some(DeletionRecord {
            profile_id: profile.id.to_string(),
            name: profile.name.clone(),
            description: profile.description.clone(),
            game_installation_id: profile.game_installation_id.to_string(),
            deleted_at: Utc::now().to_rfc3339(),
            mods,
        }))
    }

    fn load(&self, id: &ProfileId) -> AppResult<Profile> {
        self.profile_repo
            .get_profile(id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "That profile does not exist"))
    }

    fn blocked_reason(&self, profile: &Profile) -> Option<String> {
        if profile.state != ProfileState::Archived {
            return Some("Archive the profile before deleting it.".to_string());
        }
        let claims = [ResourceClaim::write(
            ResourceKind::Profile,
            profile.id.to_string(),
        )];
        if ensure_resources_available(&*self.operation_repo, &claims, None).is_err() {
            return Some(
                "A change to this profile is unfinished. Resolve it on the Activity page first."
                    .to_string(),
            );
        }
        None
    }

    pub fn preview(&self, id: &ProfileId) -> AppResult<ProfileDeletePreviewDto> {
        let profile = self.load(id)?;
        let packages: HashSet<String> = self
            .deployment_repo
            .list_deployments_for_profile(id)?
            .into_iter()
            .map(|d| d.artifact_hash.as_str().to_string())
            .collect();
        Ok(ProfileDeletePreviewDto {
            profile_id: profile.id.to_string(),
            name: profile.name.clone(),
            mod_count: self.deployment_repo.list_profile_components(id)?.len(),
            folder_bytes: self.folders.folder_size(id),
            packages_kept: packages.len(),
            blocked_reason: self.blocked_reason(&profile),
        })
    }

    pub fn delete(&self, id: &ProfileId) -> AppResult<()> {
        let profile = self.load(id)?;
        if let Some(reason) = self.blocked_reason(&profile) {
            return Err(AppError::validation("PROFILE_NOT_DELETABLE", reason));
        }
        let claims = vec![ResourceClaim::write(ResourceKind::Profile, id.to_string())];
        let _lease = self.resources.try_acquire(&claims)?;
        let _guard = self
            .instance_lock
            .acquire_guard()
            .map_err(AppError::instance_locked)?;
        if self.launcher.is_game_running(None) {
            return Err(AppError::game_running(
                "Stop Stardew Valley before deleting a profile",
            ));
        }

        // The record goes into the folder first, so it travels with it.
        if let Some(record) = self.record(&profile)? {
            let json = serde_json::to_string(&record).map_err(|e| {
                AppError::internal("Could not write the profile's record", e.to_string())
            })?;
            self.folders.write_deletion_record(id, &json)?;
        }
        let trashed = self.folders.move_to_trash(id)?;
        if let Err(error) = self.profile_repo.delete_profile(id) {
            if let Some(trashed) = &trashed {
                // Put the folder back so the profile stays whole.
                let _ = self.folders.restore_from_trash(id, trashed);
            }
            return Err(error);
        }

        // Keep a record in Activity. The profile is already gone, so failing
        // to record it must not report the deletion as failed.
        let now = Utc::now();
        let _ = self.operation_repo.create_operation(&Operation {
            id: OperationId::new(),
            kind: OperationKind::ProfileDelete,
            state: OperationState::Succeeded,
            game_installation_id: Some(profile.game_installation_id),
            profile_id: Some(profile.id),
            expected_profile_revision: Some(profile.revision),
            plan_schema_version: 1,
            plan_json: serde_json::json!({
                "profile_name": profile.name,
                "moved_to_trash": trashed.map(|p| p.display().to_string()),
            })
            .to_string(),
            progress_current: None,
            progress_total: None,
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: now,
            updated_at: now,
            completed_at: Some(now),
        });
        Ok(())
    }
}
