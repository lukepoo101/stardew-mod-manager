use crate::api::dto::{ContentPackForDto, ModDependencyDto, ModDetailsDto, ModListItemDto};
use crate::error::AppResult;
use crate::ports::repositories::{DeploymentRepository, PackageCatalogRepository};
use manager_core::ids::{ProfileComponentId, ProfileId};
use std::sync::Arc;

pub struct ModsQueries {
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
}

impl ModsQueries {
    pub fn new(
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
    ) -> Self {
        Self {
            deployment_repo,
            package_repo,
        }
    }

    pub fn list_profile_mods(&self, profile_id: &ProfileId) -> AppResult<Vec<ModListItemDto>> {
        let profile_comps = self.deployment_repo.list_profile_components(profile_id)?;
        let mut list = Vec::with_capacity(profile_comps.len());

        for pc in profile_comps {
            if let Some(comp) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            {
                let reason_str = match pc.installed_reason {
                    manager_core::deployment::InstalledReason::Direct => "direct",
                    manager_core::deployment::InstalledReason::Dependency => "dependency",
                    manager_core::deployment::InstalledReason::BundleCompanion => {
                        "bundle_companion"
                    }
                };

                list.push(ModListItemDto {
                    profile_component_id: pc.id.to_string(),
                    unique_id: comp.unique_id.to_string(),
                    name: comp.name,
                    author: comp.author,
                    version: comp.version,
                    description: comp.description,
                    enabled: pc.enabled,
                    installed_reason: reason_str.to_string(),
                    deployment_id: pc.deployment_id.to_string(),
                    artifact_hash: comp.artifact_hash.to_string(),
                    installed_at: chrono::Utc::now().to_rfc3339(),
                });
            }
        }

        Ok(list)
    }

    pub fn get_mod_details(
        &self,
        profile_component_id: &ProfileComponentId,
    ) -> AppResult<Option<ModDetailsDto>> {
        let pc = match self
            .deployment_repo
            .get_profile_component(profile_component_id)?
        {
            Some(c) => c,
            None => return Ok(None),
        };

        let comp = match self
            .package_repo
            .get_package_component(&pc.package_component_id)?
        {
            Some(c) => c,
            None => return Ok(None),
        };

        let deployment = self.deployment_repo.get_deployment(&pc.deployment_id)?;
        let acquisitions = self
            .package_repo
            .get_acquisitions_for_artifact(&comp.artifact_hash)?;
        let orig_filename = acquisitions.first().map(|a| a.original_filename.clone());

        let deps_dto: Vec<_> = comp
            .manifest
            .dependencies
            .iter()
            .map(|d| ModDependencyDto {
                unique_id: d.unique_id.to_string(),
                minimum_version: d.minimum_version.clone(),
                is_required: d.is_required,
            })
            .collect();

        let cp_dto = comp
            .manifest
            .content_pack_for
            .as_ref()
            .map(|cp| ContentPackForDto {
                unique_id: cp.unique_id.to_string(),
                minimum_version: cp.minimum_version.clone(),
            });

        Ok(Some(ModDetailsDto {
            profile_component_id: pc.id.to_string(),
            unique_id: comp.unique_id.to_string(),
            name: comp.name,
            author: comp.author,
            version: comp.version,
            description: comp.description,
            entry_dll: comp.manifest.entry_dll,
            minimum_api_version: comp.manifest.minimum_api_version,
            minimum_game_version: comp.manifest.minimum_game_version,
            update_keys: comp.manifest.update_keys,
            dependencies: deps_dto,
            content_pack_for: cp_dto,
            raw_manifest: comp.raw_manifest,
            artifact_hash: comp.artifact_hash.to_string(),
            original_filename: orig_filename,
            deployment_root_path: deployment
                .map(|d| d.root_relative_path)
                .unwrap_or_else(|| comp.relative_component_root),
            installed_at: chrono::Utc::now().to_rfc3339(),
            enabled: pc.enabled,
        }))
    }
}
