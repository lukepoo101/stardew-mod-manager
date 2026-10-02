//! Whether a profile's mod folders still hold what the manager installed.
//!
//! The baseline is the file inventory the install recorded (path, size and
//! SHA-256 of every staged file). Checking only reads. Edited `config.json`
//! files are reported separately because changing them is normal, and files
//! the manager did not install are listed without judgement: mods commonly
//! create their own. An install older than the inventory has no baseline and
//! says so rather than guessing.

use crate::api::dto::ModFilesCheckDto;
use crate::error::AppError;
use crate::error::AppResult;
use crate::ports::deployed_files::DeployedFilesPort;
use crate::ports::repositories::{
    DeploymentRepository, OperationRepository, PackageCatalogRepository, PreferencesRepository,
};
use manager_core::ids::ProfileId;
use manager_core::install::{InventoryEntry, InventoryEntryType};
use manager_core::operation::{OperationKind, OperationState};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub struct FileIntegrityService {
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    files: Arc<dyn DeployedFilesPort>,
    preferences: Option<Arc<dyn PreferencesRepository>>,
}

/// What a folder's changed files looked like when the user accepted them:
/// path -> SHA-256 at that time, or `None` for a file that was missing.
/// Accepting records the state; it does not claim the files are original.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AcceptedState {
    accepted_at: String,
    files: BTreeMap<String, Option<String>>,
}

fn accepted_key(deployment_id: &str) -> String {
    format!("accepted_files:{deployment_id}")
}

fn is_config(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("config.json"))
}

impl FileIntegrityService {
    pub fn new(
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        operation_repo: Arc<dyn OperationRepository>,
        files: Arc<dyn DeployedFilesPort>,
    ) -> Self {
        Self {
            deployment_repo,
            package_repo,
            operation_repo,
            files,
            preferences: None,
        }
    }

    /// Lets the user accept a folder's current files (see `accept_current`).
    pub fn with_preferences(mut self, preferences: Arc<dyn PreferencesRepository>) -> Self {
        self.preferences = Some(preferences);
        self
    }

    fn accepted(&self, deployment_id: &str) -> AppResult<Option<AcceptedState>> {
        let Some(preferences) = &self.preferences else {
            return Ok(None);
        };
        Ok(preferences
            .get_preference(&accepted_key(deployment_id))?
            .and_then(|json| serde_json::from_str(&json).ok()))
    }

    /// The files the install of this folder recorded, if it recorded any.
    pub fn installed_files(
        &self,
        profile_id: &ProfileId,
        deployment_id: &str,
    ) -> AppResult<Option<Vec<InventoryEntry>>> {
        let Some(deployment) = self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?
            .into_iter()
            .find(|d| d.id.to_string() == deployment_id)
        else {
            return Ok(None);
        };
        let key = (
            deployment.root_relative_path.clone(),
            deployment.artifact_hash.as_str().to_lowercase(),
        );
        Ok(self.baselines(profile_id)?.remove(&key).map(|entries| {
            entries
                .into_iter()
                .filter(|e| e.entry_type == InventoryEntryType::File)
                .collect()
        }))
    }

    /// Accepts a mod folder's changed and missing files as they are now. The
    /// mod is then shown as locally modified rather than changed, until a
    /// file differs from what was accepted. No file is touched.
    pub fn accept_current(
        &self,
        profile_id: &ProfileId,
        deployment_id: &str,
    ) -> AppResult<ModFilesCheckDto> {
        let preferences = self.preferences.as_ref().ok_or_else(|| {
            AppError::validation(
                "ACCEPT_UNAVAILABLE",
                "Accepting changes is not available here",
            )
        })?;
        let check = self
            .check_profile(profile_id)?
            .into_iter()
            .find(|c| c.deployment_id == deployment_id)
            .ok_or_else(|| {
                AppError::validation(
                    "DEPLOYMENT_NOT_FOUND",
                    "That mod folder is not in this profile",
                )
            })?;
        if check.status != "changed" && check.accepted.is_empty() {
            return Err(AppError::validation(
                "NOTHING_TO_ACCEPT",
                "This mod's files have no changes to accept",
            ));
        }
        let deployment = self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?
            .into_iter()
            .find(|d| d.id.to_string() == deployment_id)
            .ok_or_else(|| {
                AppError::validation(
                    "DEPLOYMENT_NOT_FOUND",
                    "That mod folder is not in this profile",
                )
            })?;
        let on_disk = self
            .files
            .read_folder(profile_id, &deployment.root_relative_path)?
            .unwrap_or_default();
        let mut files = BTreeMap::new();
        for path in check.modified.iter().chain(check.accepted.iter()) {
            let sha = on_disk
                .iter()
                .find(|f| &f.relative_path == path)
                .map(|f| f.sha256.to_lowercase());
            files.insert(path.clone(), sha);
        }
        for path in &check.missing {
            files.insert(path.clone(), None);
        }
        let state = AcceptedState {
            accepted_at: chrono::Utc::now().to_rfc3339(),
            files,
        };
        let json = serde_json::to_string(&state)
            .map_err(|e| AppError::internal("Could not save the accepted state", e.to_string()))?;
        preferences.set_preference(&accepted_key(deployment_id), &json)?;
        // Keep a record in Activity; failing to write it does not undo the
        // acceptance, which is already saved.
        let now = chrono::Utc::now();
        let _ = self
            .operation_repo
            .create_operation(&manager_core::operation::Operation {
                id: manager_core::ids::OperationId::new(),
                kind: OperationKind::ModFilesAccepted,
                state: OperationState::Succeeded,
                game_installation_id: None,
                profile_id: Some(*profile_id),
                expected_profile_revision: None,
                plan_schema_version: 1,
                plan_json: serde_json::json!({
                    "mod_folder_name": deployment.root_relative_path,
                    "mods": check.mods,
                    "accepted_files": state.files.keys().collect::<Vec<_>>(),
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
        self.check_profile(profile_id)?
            .into_iter()
            .find(|c| c.deployment_id == deployment_id)
            .ok_or_else(|| AppError::internal("Mod folder vanished", deployment_id.to_string()))
    }

    /// (folder, package hash) -> the inventory the most recent successful
    /// install of that folder recorded.
    fn baselines(
        &self,
        profile_id: &ProfileId,
    ) -> AppResult<HashMap<(String, String), Vec<InventoryEntry>>> {
        let mut installs: Vec<_> = self
            .operation_repo
            .list_operations_for_profile(profile_id)?
            .into_iter()
            .filter(|op| {
                op.kind == OperationKind::ModInstall && op.state == OperationState::Succeeded
            })
            .collect();
        installs.sort_by_key(|op| op.created_at);
        let mut out = HashMap::new();
        for op in installs {
            let Ok(plan) = serde_json::from_str::<serde_json::Value>(&op.plan_json) else {
                continue;
            };
            let folder = plan.get("mod_folder_name").and_then(|v| v.as_str());
            let hash = plan.get("package_hash").and_then(|v| v.as_str());
            let inventory = plan
                .get("trusted_inventory")
                .cloned()
                .and_then(|v| serde_json::from_value::<Vec<InventoryEntry>>(v).ok())
                .filter(|entries| !entries.is_empty());
            if let (Some(folder), Some(hash), Some(inventory)) = (folder, hash, inventory) {
                out.insert((folder.to_string(), hash.to_lowercase()), inventory);
            }
        }
        Ok(out)
    }

    /// "unchanged", "changed", "locally_modified" (only accepted changes),
    /// "missing_folder" or "no_record" per folder.
    pub fn check_profile(&self, profile_id: &ProfileId) -> AppResult<Vec<ModFilesCheckDto>> {
        let baselines = self.baselines(profile_id)?;
        let mut names: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            if let Some(component) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            {
                names
                    .entry(pc.deployment_id.to_string())
                    .or_default()
                    .push(component.name);
            }
        }
        let mut results = Vec::new();
        for deployment in self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?
        {
            let mut result = ModFilesCheckDto {
                deployment_id: deployment.id.to_string(),
                mods: names.remove(&deployment.id.to_string()).unwrap_or_default(),
                status: String::new(),
                missing: Vec::new(),
                modified: Vec::new(),
                added: Vec::new(),
                config_changed: Vec::new(),
                accepted: Vec::new(),
                accepted_at: None,
            };
            let key = (
                deployment.root_relative_path.clone(),
                deployment.artifact_hash.as_str().to_lowercase(),
            );
            let Some(baseline) = baselines.get(&key) else {
                result.status = "no_record".into();
                results.push(result);
                continue;
            };
            let Some(on_disk) = self
                .files
                .read_folder(profile_id, &deployment.root_relative_path)?
            else {
                result.status = "missing_folder".into();
                results.push(result);
                continue;
            };
            let disk: HashMap<&str, _> = on_disk
                .iter()
                .map(|f| (f.relative_path.as_str(), f))
                .collect();
            let mut expected = std::collections::HashSet::new();
            for entry in baseline {
                if entry.entry_type != InventoryEntryType::File {
                    continue;
                }
                let path = entry.relative_path.replace('\\', "/");
                expected.insert(path.clone());
                match disk.get(path.as_str()) {
                    None => result.missing.push(path),
                    Some(file) => {
                        let differs = file.size_bytes != entry.size_bytes
                            || entry
                                .sha256_hash
                                .as_ref()
                                .is_some_and(|h| !h.eq_ignore_ascii_case(&file.sha256));
                        if differs {
                            if is_config(&path) {
                                result.config_changed.push(path);
                            } else {
                                result.modified.push(path);
                            }
                        }
                    }
                }
            }
            result.added = on_disk
                .iter()
                .filter(|f| !expected.contains(&f.relative_path))
                .map(|f| f.relative_path.clone())
                .collect();
            // Changes the user accepted stay accepted only while the file is
            // exactly as it was then.
            if let Some(accepted) = self.accepted(&result.deployment_id)? {
                let still_accepted = |path: &String, now: Option<&str>| {
                    accepted
                        .files
                        .get(path)
                        .is_some_and(|then| then.as_deref() == now)
                };
                let (keep_missing, ok_missing): (Vec<_>, Vec<_>) = result
                    .missing
                    .drain(..)
                    .partition(|p| !still_accepted(p, None));
                let (keep_modified, ok_modified): (Vec<_>, Vec<_>) =
                    result.modified.drain(..).partition(|p| {
                        let now = disk.get(p.as_str()).map(|f| f.sha256.to_lowercase());
                        !still_accepted(p, now.as_deref())
                    });
                result.missing = keep_missing;
                result.modified = keep_modified;
                result.accepted = ok_missing.into_iter().chain(ok_modified).collect();
                result.accepted.sort();
                if !result.accepted.is_empty() {
                    result.accepted_at = Some(accepted.accepted_at.clone());
                }
            }
            result.status = if !result.missing.is_empty() || !result.modified.is_empty() {
                "changed".into()
            } else if !result.accepted.is_empty() {
                "locally_modified".into()
            } else {
                "unchanged".into()
            };
            results.push(result);
        }
        results.sort_by(|a, b| a.mods.cmp(&b.mods));
        Ok(results)
    }
}
