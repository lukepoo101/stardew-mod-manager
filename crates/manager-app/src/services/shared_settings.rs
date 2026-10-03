//! Mod settings carried inside a recipe: reading them for a curator to
//! share, telling a recipient whether theirs match (by checksum, never by
//! value), and applying a recipe's settings after a backup.

use crate::api::dto::{SettingFileHashDto, SharedSettingsDto};
use crate::error::{AppError, AppResult};
use crate::ports::config_backups::ConfigBackupsPort;
use crate::ports::deployed_files::DeployedFilesPort;
use crate::ports::repositories::{
    DeploymentRepository, OperationRepository, PackageCatalogRepository,
};
use manager_core::ids::{OperationId, ProfileId};
use manager_core::operation::{Operation, OperationKind, OperationState};
use manager_core::recipe::{sha256_hex, RecipeSetting, MAX_SETTING_BYTES};
use std::collections::HashMap;
use std::sync::Arc;

pub struct SharedSettingsService {
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    files: Arc<dyn DeployedFilesPort>,
    backups: Arc<dyn ConfigBackupsPort>,
}

impl SharedSettingsService {
    pub fn new(
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        operation_repo: Arc<dyn OperationRepository>,
        files: Arc<dyn DeployedFilesPort>,
        backups: Arc<dyn ConfigBackupsPort>,
    ) -> Self {
        Self {
            deployment_repo,
            package_repo,
            operation_repo,
            files,
            backups,
        }
    }

    /// Lower-case UniqueID -> (folder, display name) for the profile's mods.
    fn folders(&self, profile_id: &ProfileId) -> AppResult<HashMap<String, (String, String)>> {
        let mut out = HashMap::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            let Some(component) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            if let Some(deployment) = self.deployment_repo.get_deployment(&pc.deployment_id)? {
                out.insert(
                    component.unique_id.as_str().to_lowercase(),
                    (deployment.root_relative_path, component.name),
                );
            }
        }
        Ok(out)
    }

    /// The settings files of the chosen mods, as text, for a curator to put
    /// in a recipe. Files that are not text or are too large are named in
    /// `skipped` rather than included.
    pub fn read_for_sharing(
        &self,
        profile_id: &ProfileId,
        unique_ids: &[String],
    ) -> AppResult<Vec<SharedSettingsDto>> {
        let folders = self.folders(profile_id)?;
        let mut out = Vec::new();
        for unique_id in unique_ids {
            let Some((folder, _)) = folders.get(&unique_id.to_lowercase()) else {
                continue;
            };
            let mut shared = SharedSettingsDto {
                unique_id: unique_id.clone(),
                files: Vec::new(),
                skipped: Vec::new(),
            };
            for (path, bytes) in self.files.read_configs(profile_id, folder)? {
                if bytes.len() > MAX_SETTING_BYTES {
                    shared.skipped.push(format!("{path} (too large)"));
                    continue;
                }
                match String::from_utf8(bytes) {
                    Ok(text) => {
                        let setting = RecipeSetting::new(path, text);
                        shared.files.push(crate::api::dto::SharedSettingFileDto {
                            path: setting.path,
                            sha256: setting.sha256,
                            content: setting.content,
                        });
                    }
                    Err(_) => shared.skipped.push(format!("{path} (not text)")),
                }
            }
            out.push(shared);
        }
        Ok(out)
    }

    /// Checksums of every mod's settings files in the profile. Values are
    /// never returned.
    pub fn hashes(&self, profile_id: &ProfileId) -> AppResult<Vec<SettingFileHashDto>> {
        let mut out = Vec::new();
        for (unique_id, (folder, _)) in self.folders(profile_id)? {
            for (path, bytes) in self.files.read_configs(profile_id, &folder)? {
                out.push(SettingFileHashDto {
                    unique_id: unique_id.clone(),
                    path,
                    sha256: sha256_hex(&bytes),
                });
            }
        }
        out.sort_by(|a, b| (&a.unique_id, &a.path).cmp(&(&b.unique_id, &b.path)));
        Ok(out)
    }

    /// Writes a recipe's settings into one mod's folder. Every file is
    /// checked first; the mod's current settings are backed up (and the
    /// backup read back) before anything is written. Returns the backup id,
    /// if there was anything to back up.
    pub fn apply(
        &self,
        profile_id: &ProfileId,
        unique_id: &str,
        settings: &[RecipeSetting],
    ) -> AppResult<Option<String>> {
        if settings.is_empty() {
            return Err(AppError::validation(
                "NO_SETTINGS_GIVEN",
                "There are no settings to apply",
            ));
        }
        for setting in settings {
            if let Some(problem) = setting.problem() {
                return Err(AppError::validation(
                    "SHARED_SETTING_INVALID",
                    format!("Nothing was changed: {problem}"),
                ));
            }
        }
        let folders = self.folders(profile_id)?;
        let (folder, name) = folders.get(&unique_id.to_lowercase()).ok_or_else(|| {
            AppError::validation(
                "MOD_NOT_INSTALLED",
                format!("{unique_id} is not installed in this profile, so its settings cannot be applied"),
            )
        })?;
        let current = self.files.read_configs(profile_id, folder)?;
        let backup = if current.is_empty() {
            None
        } else {
            Some(self.backups.save(profile_id, unique_id, &current)?.id)
        };
        let files: Vec<(String, Vec<u8>)> = settings
            .iter()
            .map(|s| (s.path.clone(), s.content.as_bytes().to_vec()))
            .collect();
        self.files.write_files(profile_id, folder, &files)?;
        // A record in Activity; the backup above is what makes it undoable.
        let now = chrono::Utc::now();
        let _ = self.operation_repo.create_operation(&Operation {
            id: OperationId::new(),
            kind: OperationKind::ModSettingsApplied,
            state: OperationState::Succeeded,
            game_installation_id: None,
            profile_id: Some(*profile_id),
            expected_profile_revision: None,
            plan_schema_version: 1,
            plan_json: serde_json::json!({
                "mod_folder_name": folder,
                "mods": [name],
                "files": settings.iter().map(|s| &s.path).collect::<Vec<_>>(),
                "backup_id": backup,
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
        Ok(backup)
    }
}
