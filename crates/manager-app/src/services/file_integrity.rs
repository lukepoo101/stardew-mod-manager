//! Whether a profile's mod folders still hold what the manager installed.
//!
//! The baseline is the file inventory the install recorded (path, size and
//! SHA-256 of every staged file). Checking only reads. Edited `config.json`
//! files are reported separately because changing them is normal, and files
//! the manager did not install are listed without judgement: mods commonly
//! create their own. An install older than the inventory has no baseline and
//! says so rather than guessing.

use crate::api::dto::ModFilesCheckDto;
use crate::error::AppResult;
use crate::ports::deployed_files::DeployedFilesPort;
use crate::ports::repositories::{
    DeploymentRepository, OperationRepository, PackageCatalogRepository,
};
use manager_core::ids::ProfileId;
use manager_core::install::{InventoryEntry, InventoryEntryType};
use manager_core::operation::{OperationKind, OperationState};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub struct FileIntegrityService {
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    files: Arc<dyn DeployedFilesPort>,
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
        }
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
            result.status = if result.missing.is_empty() && result.modified.is_empty() {
                "unchanged".into()
            } else {
                "changed".into()
            };
            results.push(result);
        }
        results.sort_by(|a, b| a.mods.cmp(&b.mods));
        Ok(results)
    }
}
