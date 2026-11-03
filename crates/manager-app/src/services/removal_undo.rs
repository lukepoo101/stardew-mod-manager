//! Undoing a removal.
//!
//! A removal moves the mod's folder into the profile's recovery folder and
//! keeps the archive it came from. Undoing it installs that archive again
//! through the normal install (so it is checked like any install and never
//! replaces a version installed since), then writes back the `config.json`
//! files from the removed copy, so the mod comes back with the settings it
//! had rather than the archive's defaults.

use crate::api::dto::RemovalUndoResultDto;
use crate::error::{AppError, AppResult};
use crate::ports::deployed_files::DeployedFilesPort;
use crate::services::operations::RemovalPlan;
use crate::services::{OperationsService, ReinstallService};
use manager_core::deployment::DeploymentState;
use manager_core::ids::OperationId;
use manager_core::operation::{OperationKind, OperationState};

/// Said whenever a removal's kept copy is missing, however that was found.
const COPY_GONE: &str = "The mod is back with its archive's default settings: the copy kept when it was removed is no longer there (cleaned up, or left in the trash when the profile was deleted), so its old settings could not be brought back.";
use std::sync::Arc;

pub struct RemovalUndo {
    operations: Arc<OperationsService>,
    reinstall: ReinstallService,
    files: Arc<dyn DeployedFilesPort>,
}

impl RemovalUndo {
    pub fn new(
        operations: Arc<OperationsService>,
        reinstall: ReinstallService,
        files: Arc<dyn DeployedFilesPort>,
    ) -> Self {
        Self {
            operations,
            reinstall,
            files,
        }
    }

    /// Installs the removed mod again from its stored archive and restores
    /// the settings it had when it was removed.
    pub fn undo(&self, removal: &OperationId) -> AppResult<RemovalUndoResultDto> {
        let op = self
            .operations
            .operation_repo
            .get_operation(removal)?
            .ok_or_else(|| AppError::validation("OPERATION_NOT_FOUND", "Operation not found"))?;
        let profile_id = match (op.kind, op.state, op.profile_id) {
            (OperationKind::ModRemove, OperationState::Succeeded, Some(profile_id)) => profile_id,
            _ => {
                return Err(AppError::validation(
                    "NOT_AN_UNDOABLE_REMOVAL",
                    "Only a finished removal can be undone",
                ))
            }
        };
        let details = self
            .operations
            .operation_details(removal)?
            .ok_or_else(|| AppError::validation("OPERATION_NOT_FOUND", "Operation not found"))?;
        let hash = details.package_hash.clone().ok_or_else(|| {
            AppError::validation(
                "REMOVAL_PACKAGE_UNKNOWN",
                "This removal did not record which archive the mod came from, so it cannot be undone here. Install the mod again from its file.",
            )
        })?;
        let plan = RemovalPlan::parse(&op).map_err(AppError::removal_plan_invalid)?;

        // Read before installing: if the removed copy cannot be read, nothing
        // has changed yet and the user can decide.
        let settings =
            self.files
                .read_removed_configs(&profile_id, removal, &plan.deployment_rel_path)?;
        // Without the removed copy there are no settings to bring back; that
        // is said, never passed off as a mod that simply had none.
        let copy_gone =
            !self
                .files
                .removed_copy_exists(&profile_id, removal, &plan.deployment_rel_path)?;

        self.reinstall.install_stored(&profile_id, &hash)?;

        let installed: Vec<String> = details
            .changes
            .iter()
            .filter_map(|change| change.name.clone())
            .collect();
        let Some(settings) = settings else {
            // Not "it had no settings": the copy that would say is gone.
            return Ok(RemovalUndoResultDto {
                installed,
                settings_restored: 0,
                settings_problem: Some(COPY_GONE.to_string()),
            });
        };
        if settings.is_empty() {
            return Ok(RemovalUndoResultDto {
                installed,
                settings_restored: 0,
                settings_problem: copy_gone.then(|| COPY_GONE.to_string()),
            });
        }
        let folder = self
            .operations
            .deployment_repo
            .list_deployments_for_profile(&profile_id)?
            .into_iter()
            .filter(|d| {
                d.artifact_hash.as_str().eq_ignore_ascii_case(&hash)
                    && d.state != DeploymentState::Quarantined
            })
            .max_by_key(|d| d.installed_at)
            .map(|d| d.root_relative_path);
        let problem = match folder {
            Some(folder) => self
                .files
                .write_files(&profile_id, &folder, &settings)
                .err()
                .map(|error| error.summary),
            None => Some("the reinstalled mod's folder could not be found".to_string()),
        };
        Ok(RemovalUndoResultDto {
            installed,
            settings_restored: if problem.is_none() {
                settings.len() as u32
            } else {
                0
            },
            settings_problem: problem.map(|reason| {
                format!(
                    "The mod is back, but its settings were not restored ({reason}). They are still in the profile's recovery folder."
                )
            }),
        })
    }
}
