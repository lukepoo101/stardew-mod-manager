//! Reinstalling a mod cleanly from the package it was installed from.
//!
//! The retained package is re-verified against its digest before anything
//! changes. The mod's folder is then removed and installed again through the
//! normal journaled removal and install operations, so nothing is overlaid in
//! place. `config.json` files are the user's settings: they are copied from the
//! old folder first and written back into the fresh one. Files added to the
//! folder since it was installed (mods often keep their own data there) are
//! carried over the same way, when the install recorded its files. A mod that
//! was disabled is left disabled.

use crate::api::dto::{
    AddedFileDto, ReinstallPreviewDto, ReinstallResultDto, ReplaceResultDto, SettingsRestoreDto,
};
use crate::error::{AppError, AppResult};
use crate::ports::deployed_files::DeployedFilesPort;
use crate::ports::repositories::DeploymentRepository;
use crate::services::{ModsService, OperationsService, PackagesService, ToggleService};
use manager_core::deployment::DeploymentState;
use manager_core::ids::{ArtifactHash, ProfileId};
use manager_core::ids::{OperationId, ProfileComponentId};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

/// A deployment a replace removes, with what is needed to put it back.
struct Old {
    name: String,
    component: ProfileComponentId,
    folder: String,
    hash: ArtifactHash,
    enabled: bool,
    settings: Vec<(String, Vec<u8>)>,
}

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
    install_records: Option<Arc<crate::services::FileIntegrityService>>,
}

fn is_config(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("config.json"))
}

/// Whether a file added since install is kept by a reinstall unless the user
/// says otherwise. Data a mod saved is; code and a second manifest are not,
/// since they would be loaded with, or instead of, the mod's own files.
fn keep_by_default(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    if name == "manifest.json" {
        return false;
    }
    !matches!(
        name.rsplit_once('.').map(|(_, ext)| ext),
        Some("dll" | "exe" | "pdb" | "so" | "dylib" | "mdb")
    )
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
            install_records: None,
        }
    }

    /// Lets a reinstall tell files added since install from the package's
    /// own, so it can keep them.
    pub fn with_install_records(
        mut self,
        records: Arc<crate::services::FileIntegrityService>,
    ) -> Self {
        self.install_records = Some(records);
        self
    }

    /// Files in the folder now that the install did not put there (other
    /// than settings, which are kept on their own); None when there is no
    /// record of what was installed.
    fn added_since_install(
        &self,
        profile_id: &ProfileId,
        deployment: &manager_core::deployment::ProfileDeployment,
    ) -> AppResult<Option<Vec<crate::ports::deployed_files::DeployedFile>>> {
        let Some(records) = &self.install_records else {
            return Ok(None);
        };
        let Some(installed) = records.installed_files(profile_id, &deployment.id.to_string())?
        else {
            return Ok(None);
        };
        let installed: std::collections::HashSet<String> = installed
            .iter()
            .map(|entry| entry.relative_path.replace('\\', "/"))
            .collect();
        Ok(Some(
            self.files
                .read_folder_sizes(profile_id, &deployment.root_relative_path)?
                .unwrap_or_default()
                .into_iter()
                .filter(|file| !installed.contains(&file.relative_path))
                .filter(|file| !is_config(&file.relative_path))
                .collect(),
        ))
    }

    /// What a reinstall of this mod would do with files added since install.
    /// Nothing changes.
    pub fn preview_reinstall(
        &self,
        component_id: &ProfileComponentId,
    ) -> AppResult<ReinstallPreviewDto> {
        let (component, deployment) = self.record(component_id)?;
        let added = self.added_since_install(&component.profile_id, &deployment)?;
        Ok(ReinstallPreviewDto {
            no_install_record: added.is_none(),
            added: added
                .unwrap_or_default()
                .into_iter()
                .map(|file| AddedFileDto {
                    keep_by_default: keep_by_default(&file.relative_path),
                    path: file.relative_path,
                    size_bytes: file.size_bytes,
                })
                .collect(),
        })
    }

    fn record(
        &self,
        component_id: &ProfileComponentId,
    ) -> AppResult<(
        manager_core::deployment::ProfileComponent,
        manager_core::deployment::ProfileDeployment,
    )> {
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
        Ok((component, deployment))
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

    fn snapshot(
        &self,
        profile_id: &ProfileId,
        label: &str,
        source: Option<String>,
    ) -> AppResult<Option<String>> {
        let Some((preferences, package_repo)) = &self.snapshots else {
            return Ok(None);
        };
        crate::services::restore_points::record_point_with_source(
            &**preferences,
            &*self.deployment_repo,
            &**package_repo,
            profile_id,
            label,
            source,
        )
        .map(|point| Some(point.id))
    }

    /// Records on the automatic restore point an operation it was taken
    /// before. Losing this note loses only the link, never the point.
    fn note(
        &self,
        profile_id: &ProfileId,
        point: &Option<String>,
        operation: &OperationId,
        change: &str,
    ) {
        if let Some((preferences, _)) = &self.snapshots {
            let _ = crate::services::operation_labels::label_operation(
                &**preferences,
                operation,
                change,
            );
            if let Some(point) = point {
                let _ = crate::services::restore_points::note_operation(
                    &**preferences,
                    profile_id,
                    point,
                    operation,
                );
            }
        }
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

    /// Reinstalls with the default choice for added files (see
    /// [`ReinstallService::preview_reinstall`]).
    pub fn reinstall(&self, component_id: &ProfileComponentId) -> AppResult<ReinstallResultDto> {
        self.reinstall_keeping(component_id, None)
    }

    /// Reinstalls a mod. `keep` names the added files to carry over; `None`
    /// keeps the ones kept by default. Files not kept are not deleted: they
    /// stay in the copy the removal keeps in the recovery folder.
    pub fn reinstall_keeping(
        &self,
        component_id: &ProfileComponentId,
        keep: Option<&[String]>,
    ) -> AppResult<ReinstallResultDto> {
        let (component, deployment) = self.record(component_id)?;
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
        let added = self.added_since_install(&profile_id, &deployment)?;
        let (kept, left_out): (Vec<String>, Vec<String>) = added
            .iter()
            .flatten()
            .map(|file| file.relative_path.clone())
            .partition(|path| match keep {
                Some(chosen) => chosen.iter().any(|c| c == path),
                None => keep_by_default(path),
            });

        let restore_point = self.snapshot(&profile_id, "Before reinstalling a mod", None)?;
        let change = format!("Reinstalling {}", deployment.root_relative_path);
        let settings_backup = self.back_up(&profile_id, component_id, &settings)?;
        // Removal works on live mods; a disabled one is disabled again below.
        if !was_enabled {
            self.toggle.set_enabled(component_id, true)?;
        }
        let removal = self.mods.prepare_removal(component_id)?;
        let removal_id = operation_id(&removal.operation_id)?;
        self.operations.commit_operation(&removal_id)?;
        self.note(&profile_id, &restore_point, &removal_id, &change);

        let removed_copy = self.files.removed_copy_location(
            &profile_id,
            &removal_id,
            &deployment.root_relative_path,
        );
        let reinstall_failed = |error: AppError| {
            AppError::validation(
                "REINSTALL_INCOMPLETE",
                format!(
                    "The old copy was removed, but installing it again failed: {}. Install the same archive again from the Mods page.{}",
                    error.summary,
                    removed_copy
                        .as_deref()
                        .map(|path| format!(" The removed folder, with its settings and any files added to it, is kept in {path}."))
                        .unwrap_or_default()
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
        let install_id = operation_id(&preview.operation_id)?;
        self.operations
            .commit_operation(&install_id)
            .map_err(reinstall_failed)?;
        self.note(&profile_id, &restore_point, &install_id, &change);

        let new_deployment = self
            .deployment_repo
            .list_deployments_for_profile(&profile_id)?
            .into_iter()
            // The removed deployment's record can remain, as can older ones
            // set aside by replaces; the new one is live and differs.
            .find(|d| {
                d.artifact_hash == hash
                    && d.id != deployment.id
                    && d.state != DeploymentState::Quarantined
            })
            .ok_or_else(|| AppError::internal("Reinstalled mod not found", hash.to_string()))?;
        self.files
            .write_files(&profile_id, &new_deployment.root_relative_path, &settings)?;
        // Copied from the removed folder rather than held in memory, so a
        // large cache costs nothing and a failure leaves them in place there.
        let kept_added = if kept.is_empty() {
            Vec::new()
        } else {
            self.files.copy_from_removed(
                &profile_id,
                &removal_id,
                &new_deployment.root_relative_path,
                &kept,
            )?
        };

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
            kept_added,
            removed_copy: (!left_out.is_empty()).then_some(removed_copy).flatten(),
            left_out,
            no_install_record: added.is_none(),
            left_disabled: !was_enabled,
            settings_backup,
            restore_point,
        })
    }

    /// Writes a settings backup back into the current folder of the mod with
    /// this UniqueID. The settings it replaces are backed up first, when they
    /// differ, so a wrong pick can be undone. Nothing else in the folder
    /// changes.
    pub fn restore_settings(
        &self,
        profile_id: &ProfileId,
        unique_id: &str,
        backup_id: &str,
    ) -> AppResult<SettingsRestoreDto> {
        let (backups, package_repo) = self.backups.as_ref().ok_or_else(|| {
            AppError::validation(
                "BACKUPS_UNAVAILABLE",
                "Settings backups are not available here",
            )
        })?;
        if !backup_id
            .to_lowercase()
            .starts_with(&format!("{}/", unique_id.to_lowercase()))
        {
            return Err(AppError::validation(
                "BACKUP_NOT_FOR_MOD",
                "That backup belongs to a different mod",
            ));
        }
        let deployments: std::collections::HashMap<_, _> = self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?
            .into_iter()
            .filter(|d| d.state != DeploymentState::Quarantined)
            .map(|d| (d.id, d))
            .collect();
        let mut folder = None;
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            let Some(deployment) = deployments.get(&pc.deployment_id) else {
                continue;
            };
            let matches = package_repo
                .get_package_component(&pc.package_component_id)?
                .is_some_and(|c| c.unique_id.as_str().eq_ignore_ascii_case(unique_id));
            if matches {
                folder = Some(deployment.root_relative_path.clone());
                break;
            }
        }
        let folder = folder.ok_or_else(|| {
            AppError::validation(
                "COMPONENT_NOT_FOUND",
                "That mod is not installed in this profile",
            )
        })?;
        let files = backups.load(profile_id, backup_id)?;
        let current = self.files.read_configs(profile_id, &folder)?;
        let differs = current
            .iter()
            .any(|(path, bytes)| !files.iter().any(|(p, b)| p == path && b == bytes));
        let saved_current = if differs {
            Some(backups.save(profile_id, unique_id, &current)?.id)
        } else {
            None
        };
        self.files.write_files(profile_id, &folder, &files)?;
        Ok(SettingsRestoreDto {
            files: files.into_iter().map(|(path, _)| path).collect(),
            saved_current,
        })
    }

    /// Installs a package the manager already stores into a profile, after
    /// checking it is intact, through the normal journaled install.
    pub fn install_stored(&self, profile_id: &ProfileId, artifact_hash: &str) -> AppResult<()> {
        let hash = ArtifactHash::parse(artifact_hash.to_string()).map_err(|_| {
            AppError::validation("PACKAGE_INVALID", "That is not a package checksum")
        })?;
        if !self.packages.has_artifact(&hash) || !self.packages.verify_artifact(&hash)? {
            return Err(AppError::validation(
                "PACKAGE_NOT_STORED",
                "That exact package is not stored intact here; get the file and install it from the Mods page",
            ));
        }
        let package = self.packages.get_artifact_path(&hash)?;
        self.install(profile_id, &package).map(|_| ())
    }

    /// Installs a stored package to satisfy another mod's requirement, and
    /// records that as the reason it is installed.
    pub fn install_stored_as_dependency(
        &self,
        profile_id: &ProfileId,
        artifact_hash: &str,
    ) -> AppResult<()> {
        self.install_stored(profile_id, artifact_hash)?;
        let deployments: std::collections::HashSet<_> = self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?
            .into_iter()
            .filter(|d| d.artifact_hash.as_str().eq_ignore_ascii_case(artifact_hash))
            .map(|d| d.id)
            .collect();
        for mut component in self.deployment_repo.list_profile_components(profile_id)? {
            if deployments.contains(&component.deployment_id) {
                component.installed_reason = manager_core::deployment::InstalledReason::Dependency;
                self.deployment_repo.save_profile_component(&component)?;
            }
        }
        Ok(())
    }

    /// Installs a package's components and commits it, or explains why not.
    pub(crate) fn install(&self, profile_id: &ProfileId, package: &Path) -> AppResult<OperationId> {
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
        Ok(id)
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
                name: replacement.name.clone(),
                component: cid,
                folder: deployment.root_relative_path.clone(),
                hash: deployment.artifact_hash.clone(),
                enabled: component.enabled,
                settings: self
                    .files
                    .read_configs(profile_id, &deployment.root_relative_path)?,
            });
        }

        // A downloaded update says where the new version came from, so the
        // point can be told apart from one taken before a chosen archive:
        // briefly in its label, which has a length limit, and in full with it.
        let origin = self.snapshots.as_ref().and_then(|(_, package_repo)| {
            crate::services::provenance::github_origin(&**package_repo, &hash)
        });
        let changing = preview
            .replaces
            .iter()
            .map(|r| format!("{} {}", r.name, r.installed_version))
            .collect::<Vec<_>>()
            .join(", ");
        let restore_point = self.snapshot(
            profile_id,
            &restore_point_label(&changing, origin.as_ref()),
            origin.map(|o| format!("Taken before updating from {}", o.describe())),
        )?;
        let change = format!(
            "Changing {}",
            preview
                .replaces
                .iter()
                .map(|r| format!(
                    "{} {} → {}",
                    r.name, r.installed_version, r.incoming_version
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut settings_backup = None;
        for old in &olds {
            if let Some(id) = self.back_up(profile_id, &old.component, &old.settings)? {
                settings_backup.get_or_insert(id);
            }
        }
        // Remove the old versions one at a time. If one cannot be removed,
        // the removal itself leaves it installed (or enters recovery, which
        // only ever puts it back), and the ones already removed are put back,
        // so a failed replace never leaves the profile without the mod.
        let mut removed: Vec<&Old> = Vec::new();
        for old in &olds {
            let removal = (|| {
                if !old.enabled {
                    self.toggle.set_enabled(&old.component, true)?;
                }
                let removal = self.mods.prepare_removal(&old.component)?;
                let removal_id = operation_id(&removal.operation_id)?;
                self.operations.commit_operation(&removal_id)?;
                self.note(profile_id, &restore_point, &removal_id, &change);
                Ok::<_, AppError>(())
            })();
            if let Err(error) = removal {
                // Still installed: leave it as it was found.
                if !old.enabled {
                    let _ = self.toggle.set_enabled(&old.component, false);
                }
                let put_back = self.put_back(profile_id, &restore_point, &change, &removed);
                return Err(AppError {
                    code: "REPLACE_FAILED".to_string(),
                    summary: format!(
                        "{} could not be replaced, because its old version could not be removed: {}{}",
                        old.name,
                        error.summary,
                        put_back
                    ),
                    ..error
                });
            }
            removed.push(old);
        }

        let installed = self.install(profile_id, &package);
        if let Ok(install_id) = &installed {
            self.note(profile_id, &restore_point, install_id, &change);
        }
        if let Err(error) = installed {
            let put_back = self.put_back(profile_id, &restore_point, &change, &removed);
            return Err(AppError::validation(
                "REPLACE_FAILED",
                format!(
                    "The new version could not be installed: {}{}",
                    error.summary, put_back
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
        // A mod that was turned off stays off, even when it is replaced
        // together with mods that were on.
        let left_disabled = olds.iter().any(|o| !o.enabled);
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

    /// Installs removed old versions again, with their settings and enabled
    /// state, and says in words which ones are back and which are not.
    fn put_back(
        &self,
        profile_id: &ProfileId,
        restore_point: &Option<String>,
        change: &str,
        removed: &[&Old],
    ) -> String {
        if removed.is_empty() {
            return ". Nothing else was changed".to_string();
        }
        let mut back = Vec::new();
        let mut missing = Vec::new();
        for old in removed {
            let result = (|| {
                let path = self.packages.get_artifact_path(&old.hash)?;
                let put_back = self.install(profile_id, &path)?;
                self.note(profile_id, restore_point, &put_back, change);
                if let Some(new) = self.find_deployment(profile_id, &old.hash)? {
                    self.files.write_files(profile_id, &new.0, &old.settings)?;
                    if !old.enabled {
                        self.toggle.set_enabled(&new.1, false)?;
                    }
                }
                Ok::<_, AppError>(())
            })();
            match result {
                Ok(()) => back.push(old.name.clone()),
                Err(error) => missing.push(format!("{} ({})", old.name, error.summary)),
            }
        }
        let mut words = String::new();
        if !back.is_empty() {
            words.push_str(&format!(
                ". The previous version was put back for {}",
                back.join(", ")
            ));
        }
        if !missing.is_empty() {
            words.push_str(&format!(
                ". The previous version could not be put back for {}",
                missing.join(", ")
            ));
            if restore_point.is_some() {
                words.push_str("; the restore point saved before this change has it");
            }
        }
        words
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

/// "Before changing Stardew UI 0.6.0", and for a downloaded update "…, from
/// GitHub owner/name v0.6.1", within the label limit: the mods' names are
/// shortened first so where it came from stays.
fn restore_point_label(
    changing: &str,
    origin: Option<&crate::services::provenance::GitHubOrigin>,
) -> String {
    use crate::services::restore_points::MAX_LABEL_CHARS;
    let suffix = origin
        .map(|o| format!(", from GitHub {} {}", o.repo, o.tag))
        .unwrap_or_default();
    let room = MAX_LABEL_CHARS
        .saturating_sub("Before changing ".len() + suffix.chars().count())
        .clamp(8, 60);
    let mut names: String = changing.chars().take(room).collect();
    if changing.chars().count() > room {
        names.pop();
        names.push('…');
    }
    format!("Before changing {names}{suffix}")
        .chars()
        .take(MAX_LABEL_CHARS)
        .collect()
}

#[cfg(test)]
mod label_tests {
    use super::*;
    use crate::services::provenance::GitHubOrigin;
    use crate::services::restore_points::MAX_LABEL_CHARS;

    fn origin(repo: &str) -> GitHubOrigin {
        GitHubOrigin {
            repo: repo.into(),
            tag: "v0.6.1".into(),
            asset_name: "StardewUI.0.6.1.zip".into(),
            checksum_verified: true,
        }
    }

    #[test]
    fn a_label_names_the_github_release_within_the_limit() {
        assert_eq!(
            restore_point_label("Stardew UI 0.6.0", None),
            "Before changing Stardew UI 0.6.0"
        );
        assert_eq!(
            restore_point_label("Stardew UI 0.6.0", Some(&origin("focustense/StardewUI"))),
            "Before changing Stardew UI 0.6.0, from GitHub focustense/StardewUI v0.6.1"
        );
        let long = restore_point_label(
            &"Very Long Mod Name 1.0.0, ".repeat(5),
            Some(&origin("someone/long-repository-name")),
        );
        assert!(long.chars().count() <= MAX_LABEL_CHARS, "{long}");
        assert!(long.contains('…'), "{long}");
        assert!(
            long.ends_with("someone/long-repository-name v0.6.1"),
            "{long}"
        );
        let huge = restore_point_label("A 1.0", Some(&origin(&"x".repeat(200))));
        assert_eq!(huge.chars().count(), MAX_LABEL_CHARS);
    }
}
