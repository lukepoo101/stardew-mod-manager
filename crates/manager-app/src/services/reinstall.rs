//! Reinstalling a mod cleanly from the package it was installed from.
//!
//! The retained package is re-verified against its digest before anything
//! changes. The mod's folder is then removed and installed again through the
//! normal journaled removal and install operations, so nothing is overlaid in
//! place. `config.json` files are the user's settings: they are copied from the
//! old folder first and written back into the fresh one. A mod that was
//! disabled is left disabled.

use crate::api::dto::ReinstallResultDto;
use crate::error::{AppError, AppResult};
use crate::ports::deployed_files::DeployedFilesPort;
use crate::ports::repositories::DeploymentRepository;
use crate::services::{ModsService, OperationsService, PackagesService, ToggleService};
use manager_core::ids::{OperationId, ProfileComponentId};
use std::str::FromStr;
use std::sync::Arc;

pub struct ReinstallService {
    deployment_repo: Arc<dyn DeploymentRepository>,
    packages: Arc<PackagesService>,
    mods: Arc<ModsService>,
    operations: Arc<OperationsService>,
    toggle: Arc<ToggleService>,
    files: Arc<dyn DeployedFilesPort>,
}

fn operation_id(raw: &str) -> AppResult<OperationId> {
    OperationId::from_str(raw)
        .map_err(|e| AppError::internal("Operation id was not readable", e.to_string()))
}

impl ReinstallService {
    pub fn new(
        deployment_repo: Arc<dyn DeploymentRepository>,
        packages: Arc<PackagesService>,
        mods: Arc<ModsService>,
        operations: Arc<OperationsService>,
        toggle: Arc<ToggleService>,
        files: Arc<dyn DeployedFilesPort>,
    ) -> Self {
        Self {
            deployment_repo,
            packages,
            mods,
            operations,
            toggle,
            files,
        }
    }

    pub fn reinstall(&self, component_id: &ProfileComponentId) -> AppResult<ReinstallResultDto> {
        let component = self
            .deployment_repo
            .get_profile_component(component_id)?
            .ok_or_else(|| {
                AppError::validation("COMPONENT_NOT_FOUND", "That mod is not in any profile")
            })?;
        let deployment = self
            .deployment_repo
            .get_deployment(&component.deployment_id)?
            .ok_or_else(|| {
                AppError::validation("DEPLOYMENT_NOT_FOUND", "The mod's files are not recorded")
            })?;
        let profile_id = component.profile_id;
        let hash = deployment.artifact_hash.clone();

        // Nothing changes unless the package is there and intact.
        if !self.packages.has_artifact(&hash) {
            return Err(AppError::validation(
                "PACKAGE_NOT_RETAINED",
                "The archive this mod was installed from is no longer kept, so it cannot be reinstalled from it",
            ));
        }
        if !self.packages.verify_artifact(&hash)? {
            return Err(AppError::validation(
                "PACKAGE_DAMAGED",
                "The stored archive no longer matches its checksum, so it was not used",
            ));
        }
        let package = self.packages.get_artifact_path(&hash)?;
        let was_enabled = component.enabled;
        let settings = self
            .files
            .read_configs(&profile_id, &deployment.root_relative_path)?;

        // Removal works on live mods; a disabled one is disabled again below.
        if !was_enabled {
            self.toggle.set_enabled(component_id, true)?;
        }
        let removal = self.mods.prepare_removal(component_id)?;
        self.operations
            .commit_operation(&operation_id(&removal.operation_id)?)?;

        let reinstall_failed = |error: AppError| {
            AppError::validation(
                "REINSTALL_INCOMPLETE",
                format!(
                    "The old copy was removed, but installing it again failed: {}. Install the same archive again from the Mods page.",
                    error.summary
                ),
            )
        };
        let preview = self
            .mods
            .prepare_install(&profile_id, &package)
            .map_err(reinstall_failed)?;
        if !preview.blockers.is_empty() {
            let _ = self
                .operations
                .cancel_operation(&operation_id(&preview.operation_id)?);
            return Err(reinstall_failed(AppError::validation(
                "INSTALL_BLOCKED",
                preview.blockers.join(" "),
            )));
        }
        self.operations
            .commit_operation(&operation_id(&preview.operation_id)?)
            .map_err(reinstall_failed)?;

        let new_deployment = self
            .deployment_repo
            .list_deployments_for_profile(&profile_id)?
            .into_iter()
            // The removed deployment's record can remain; the new one differs.
            .find(|d| d.artifact_hash == hash && d.id != deployment.id)
            .ok_or_else(|| AppError::internal("Reinstalled mod not found", hash.to_string()))?;
        self.files
            .write_files(&profile_id, &new_deployment.root_relative_path, &settings)?;

        if !was_enabled {
            if let Some(first) = self
                .deployment_repo
                .list_profile_components(&profile_id)?
                .into_iter()
                .find(|pc| pc.deployment_id == new_deployment.id)
            {
                self.toggle.set_enabled(&first.id, false)?;
            }
        }

        Ok(ReinstallResultDto {
            mods: preview
                .detected_components
                .iter()
                .map(|c| format!("{} {}", c.name, c.version))
                .collect(),
            kept_settings: settings.into_iter().map(|(path, _)| path).collect(),
            left_disabled: !was_enabled,
        })
    }
}
