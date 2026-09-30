//! Reinstalling a mod cleanly from the package it was installed from.
//!
//! The retained package is re-verified against its digest before anything
//! changes. The mod's folder is then removed and installed again through the
//! normal journaled removal and install operations, so nothing is overlaid in
//! place. `config.json` files are the user's settings: they are copied from the
//! old folder first and written back into the fresh one. A mod that was
//! disabled is left disabled.

use crate::api::dto::{ReinstallResultDto, ReplaceResultDto};
use crate::error::{AppError, AppResult};
use crate::ports::deployed_files::DeployedFilesPort;
use crate::ports::repositories::DeploymentRepository;
use crate::services::{ModsService, OperationsService, PackagesService, ToggleService};
use manager_core::ids::{ArtifactHash, ProfileId};
use manager_core::ids::{OperationId, ProfileComponentId};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

pub struct ReinstallService {
    deployment_repo: Arc<dyn DeploymentRepository>,
    packages: Arc<PackagesService>,
    mods: Arc<ModsService>,
    operations: Arc<OperationsService>,
    toggle: Arc<ToggleService>,
    files: Arc<dyn DeployedFilesPort>,
    backups: Option<(
        Arc<dyn crate::ports::config_backups::ConfigBackupsPort>,
        Arc<dyn crate::ports::repositories::PackageCatalogRepository>,
    )>,
    snapshots: Option<(
        Arc<dyn crate::ports::repositories::PreferencesRepository>,
        Arc<dyn crate::ports::repositories::PackageCatalogRepository>,
    )>,
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
            backups: None,
            snapshots: None,
        }
    }

    /// Saves an automatic restore point before each replace or reinstall; if
    /// it cannot be saved, the change does not start.
    pub fn with_restore_points(
        mut self,
        preferences: Arc<dyn crate::ports::repositories::PreferencesRepository>,
        package_repo: Arc<dyn crate::ports::repositories::PackageCatalogRepository>,
    ) -> Self {
        self.snapshots = Some((preferences, package_repo));
        self
    }

    fn snapshot(&self, profile_id: &ProfileId, label: &str) -> AppResult<Option<String>> {
        let Some((preferences, package_repo)) = &self.snapshots else {
            return Ok(None);
        };
        crate::services::restore_points::record_point(
            &**preferences,
            &*self.deployment_repo,
            &**package_repo,
            profile_id,
            label,
        )
        .map(|point| Some(point.id))
    }

    /// Saves each mod's settings to a verified backup before it is replaced
    /// or reinstalled.
    pub fn with_config_backups(
        mut self,
        backups: Arc<dyn crate::ports::config_backups::ConfigBackupsPort>,
        package_repo: Arc<dyn crate::ports::repositories::PackageCatalogRepository>,
    ) -> Self {
        self.backups = Some((backups, package_repo));
        self
    }

    /// Saves settings before a change; a failed backup stops the change.
    fn back_up(
        &self,
        profile_id: &ProfileId,
        component_id: &ProfileComponentId,
        settings: &[(String, Vec<u8>)],
    ) -> AppResult<Option<String>> {
        let Some((backups, package_repo)) = &self.backups else {
            return Ok(None);
        };
        if settings.is_empty() {
            return Ok(None);
        }
        let component = self
            .deployment_repo
            .get_profile_component(component_id)?
            .and_then(|pc| {
                package_repo
                    .get_package_component(&pc.package_component_id)
                    .ok()
                    .flatten()
            })
            .ok_or_else(|| {
                AppError::validation("COMPONENT_NOT_FOUND", "That mod is not in any profile")
            })?;
        Ok(Some(
            backups
                .save(profile_id, component.unique_id.as_str(), settings)?
                .id,
        ))
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

        let restore_point = self.snapshot(&profile_id, "Before reinstalling a mod")?;
        let settings_backup = self.back_up(&profile_id, component_id, &settings)?;
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
            settings_backup,
            restore_point,
        })
    }

    /// Installs a package's components and commits it, or explains why not.
    pub(crate) fn install(&self, profile_id: &ProfileId, package: &Path) -> AppResult<()> {
        let preview = self.mods.prepare_install(profile_id, package)?;
        let id = operation_id(&preview.operation_id)?;
        if !preview.blockers.is_empty() {
            let _ = self.operations.cancel_operation(&id);
            return Err(AppError::validation(
                "INSTALL_BLOCKED",
                preview.blockers.join(" "),
            ));
        }
        self.operations.commit_operation(&id)?;
        Ok(())
    }

    /// Replaces installed mods with the version in a retained package: an
    /// upgrade, a downgrade or the same version again. The old copies are
    /// removed through the normal journaled removal with their `config.json`
    /// kept, and the new package is installed. If the new package cannot be
    /// installed, the old packages are installed again, so the profile is
    /// never left without the mod.
    pub fn replace(
        &self,
        profile_id: &ProfileId,
        artifact_hash: &str,
    ) -> AppResult<ReplaceResultDto> {
        let hash = ArtifactHash::parse(artifact_hash.to_string()).map_err(|_| {
            AppError::validation("PACKAGE_INVALID", "That is not a package checksum")
        })?;
        if !self.packages.has_artifact(&hash) || !self.packages.verify_artifact(&hash)? {
            return Err(AppError::validation(
                "PACKAGE_DAMAGED",
                "The archive is not stored intact, so nothing was replaced",
            ));
        }
        let package = self.packages.get_artifact_path(&hash)?;

        // Work out what it replaces from a fresh inspection, not the caller.
        let preview = self.mods.prepare_install(profile_id, &package)?;
        let _ = self
            .operations
            .cancel_operation(&operation_id(&preview.operation_id)?);
        if preview.replaces.is_empty() {
            return Err(AppError::validation(
                "NOTHING_TO_REPLACE",
                "No installed mod has the same UniqueID; install it normally instead",
            ));
        }

        // The deployments that will go, with what is needed to put them back.
        struct Old {
            component: ProfileComponentId,
            folder: String,
            hash: ArtifactHash,
            enabled: bool,
            settings: Vec<(String, Vec<u8>)>,
        }
        let mut olds: Vec<Old> = Vec::new();
        for replacement in &preview.replaces {
            let cid = ProfileComponentId::from_str(&replacement.profile_component_id)
                .map_err(|e| AppError::internal("Component id was not readable", e.to_string()))?;
            let component = self
                .deployment_repo
                .get_profile_component(&cid)?
                .ok_or_else(|| AppError::validation("COMPONENT_NOT_FOUND", "That mod is gone"))?;
            let deployment = self
                .deployment_repo
                .get_deployment(&component.deployment_id)?
                .ok_or_else(|| {
                    AppError::validation("DEPLOYMENT_NOT_FOUND", "The mod's files are not recorded")
                })?;
            if olds
                .iter()
                .any(|o| o.folder == deployment.root_relative_path)
            {
                continue;
            }
            if !self.packages.has_artifact(&deployment.artifact_hash) {
                return Err(AppError::validation(
                    "PACKAGE_NOT_RETAINED",
                    format!(
                        "The archive {} was installed from is no longer kept, so it could not be put back if the new version failed. Remove it first, then install the new version.",
                        replacement.name
                    ),
                ));
            }
            olds.push(Old {
                component: cid,
                folder: deployment.root_relative_path.clone(),
                hash: deployment.artifact_hash.clone(),
                enabled: component.enabled,
                settings: self
                    .files
                    .read_configs(profile_id, &deployment.root_relative_path)?,
            });
        }

        let restore_point = self.snapshot(
            profile_id,
            &format!(
                "Before changing {}",
                preview
                    .replaces
                    .iter()
                    .map(|r| format!("{} {}", r.name, r.installed_version))
                    .collect::<Vec<_>>()
                    .join(", ")
                    .chars()
                    .take(60)
                    .collect::<String>()
            ),
        )?;
        let mut settings_backup = None;
        for old in &olds {
            if let Some(id) = self.back_up(profile_id, &old.component, &old.settings)? {
                settings_backup.get_or_insert(id);
            }
        }
        for old in &olds {
            if !old.enabled {
                self.toggle.set_enabled(&old.component, true)?;
            }
            let removal = self.mods.prepare_removal(&old.component)?;
            self.operations
                .commit_operation(&operation_id(&removal.operation_id)?)?;
        }

        if let Err(error) = self.install(profile_id, &package) {
            // Put the old versions back as they were.
            for old in &olds {
                if let Ok(path) = self.packages.get_artifact_path(&old.hash) {
                    if self.install(profile_id, &path).is_ok() {
                        if let Some(new) = self.find_deployment(profile_id, &old.hash)? {
                            let _ = self.files.write_files(profile_id, &new.0, &old.settings);
                            if !old.enabled {
                                let _ = self.toggle.set_enabled(&new.1, false);
                            }
                        }
                    }
                }
            }
            return Err(AppError::validation(
                "REPLACE_FAILED",
                format!(
                    "The new version could not be installed, so the previous one was put back: {}",
                    error.summary
                ),
            ));
        }

        let mut kept = Vec::new();
        let (folder, first_component) = self
            .find_deployment(profile_id, &hash)?
            .ok_or_else(|| AppError::internal("Replacement not found", hash.to_string()))?;
        // Settings only map onto a single old folder unambiguously.
        if let [old] = olds.as_slice() {
            self.files.write_files(profile_id, &folder, &old.settings)?;
            kept = old.settings.iter().map(|(path, _)| path.clone()).collect();
        }
        let left_disabled = olds.iter().all(|o| !o.enabled);
        if left_disabled {
            self.toggle.set_enabled(&first_component, false)?;
        }
        Ok(ReplaceResultDto {
            replaced: preview.replaces,
            kept_settings: kept,
            left_disabled,
            settings_backup,
            restore_point,
        })
    }

    /// The live deployment installed from a package, and one of its components.
    fn find_deployment(
        &self,
        profile_id: &ProfileId,
        hash: &ArtifactHash,
    ) -> AppResult<Option<(String, ProfileComponentId)>> {
        let components = self.deployment_repo.list_profile_components(profile_id)?;
        for deployment in self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?
        {
            if &deployment.artifact_hash != hash {
                continue;
            }
            if let Some(pc) = components
                .iter()
                .find(|pc| pc.deployment_id == deployment.id)
            {
                return Ok(Some((deployment.root_relative_path, pc.id)));
            }
        }
        Ok(None)
    }
}
