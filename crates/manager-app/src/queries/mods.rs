use crate::api::dto::{
    ContentPackForDto, ModDependencyDto, ModDependentDto, ModDetailsDto, ModListItemDto,
    ModRelationsDto, ModRequirementDto,
};
use crate::error::AppResult;
use crate::ports::repositories::{DeploymentRepository, PackageCatalogRepository};
use manager_core::dependency::relations::{relations, EdgeKind, EdgeStatus, RelationMod};
use manager_core::deployment::InstalledReason;
use manager_core::ids::{ProfileComponentId, ProfileId};
use std::collections::HashMap;
use std::sync::Arc;

pub struct ModsQueries {
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    history: Option<Arc<dyn crate::ports::repositories::OperationRepository>>,
}

impl ModsQueries {
    pub fn new(
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
    ) -> Self {
        Self {
            deployment_repo,
            package_repo,
            history: None,
        }
    }

    /// Lets mod details list the versions a mod had before, from the removals
    /// recorded in operation history.
    pub fn with_history(
        mut self,
        operations: Arc<dyn crate::ports::repositories::OperationRepository>,
    ) -> Self {
        self.history = Some(operations);
        self
    }

    fn earlier_versions(
        &self,
        profile_id: &ProfileId,
        unique_id: &str,
        current_version: &str,
    ) -> AppResult<Vec<String>> {
        let Some(history) = &self.history else {
            return Ok(Vec::new());
        };
        let mut ops = history.list_operations_for_profile(profile_id)?;
        ops.sort_by_key(|op| std::cmp::Reverse(op.created_at));
        let mut out: Vec<String> = Vec::new();
        for op in ops {
            for effect in history.list_operation_effects(&op.id)? {
                if effect.change_kind != "ProfileComponentRemoved" {
                    continue;
                }
                let Some(snapshot) = effect
                    .before_json
                    .as_deref()
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
                else {
                    continue;
                };
                let same = snapshot
                    .get("unique_id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id.eq_ignore_ascii_case(unique_id));
                let version = snapshot.get("version").and_then(|v| v.as_str());
                if let (true, Some(version)) = (same, version) {
                    let entry = format!(
                        "{version}, removed {}",
                        effect.occurred_at.format("%Y-%m-%d")
                    );
                    if version != current_version
                        && !out.iter().any(|e| e.starts_with(&format!("{version},")))
                    {
                        out.push(entry);
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn list_profile_mods(&self, profile_id: &ProfileId) -> AppResult<Vec<ModListItemDto>> {
        let profile_comps = self.deployment_repo.list_profile_components(profile_id)?;
        let mut list = Vec::with_capacity(profile_comps.len());
        let installed_at: HashMap<_, _> = self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?
            .into_iter()
            .map(|d| (d.id, d.installed_at.to_rfc3339()))
            .collect();

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
                    installed_at: installed_at
                        .get(&pc.deployment_id)
                        .cloned()
                        .unwrap_or_default(),
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
        let source = acquisitions.first().map(|a| {
            match a.source {
                manager_core::package::AcquisitionSource::LocalFile => "A file on this computer",
                manager_core::package::AcquisitionSource::DirectUrl => {
                    "Downloaded from a web address"
                }
                manager_core::package::AcquisitionSource::Provider => "Downloaded from a mod site",
                manager_core::package::AcquisitionSource::ManualReference => "Added by hand",
            }
            .to_string()
        });
        let acquired_at = acquisitions.first().map(|a| a.acquired_at.to_rfc3339());

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

        let earlier_versions =
            self.earlier_versions(&pc.profile_id, comp.unique_id.as_str(), &comp.version)?;
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
            source,
            acquired_at,
            earlier_versions,
            installed_at: deployment
                .as_ref()
                .map(|d| d.installed_at.to_rfc3339())
                .unwrap_or_default(),
            deployment_root_path: deployment
                .map(|d| d.root_relative_path)
                .unwrap_or_else(|| comp.relative_component_root),
            enabled: pc.enabled,
        }))
    }

    /// Why a mod is installed, what it needs and what needs it.
    pub fn get_mod_relations(
        &self,
        profile_component_id: &ProfileComponentId,
    ) -> AppResult<Option<ModRelationsDto>> {
        let Some(selected) = self
            .deployment_repo
            .get_profile_component(profile_component_id)?
        else {
            return Ok(None);
        };
        let mut mods = Vec::new();
        let mut names: HashMap<String, (String, String, bool)> = HashMap::new();
        for pc in self
            .deployment_repo
            .list_profile_components(&selected.profile_id)?
        {
            let Some(comp) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            let mut dependencies: Vec<(String, Option<String>, EdgeKind)> = comp
                .manifest
                .dependencies
                .iter()
                .map(|d| {
                    (
                        d.unique_id.to_string(),
                        d.minimum_version.clone(),
                        if d.is_required {
                            EdgeKind::Required
                        } else {
                            EdgeKind::Optional
                        },
                    )
                })
                .collect();
            if let Some(host) = &comp.manifest.content_pack_for {
                dependencies.push((
                    host.unique_id.to_string(),
                    host.minimum_version.clone(),
                    EdgeKind::ContentPackFor,
                ));
            }
            names.insert(
                pc.id.to_string(),
                (comp.name.clone(), comp.unique_id.to_string(), pc.enabled),
            );
            mods.push(RelationMod {
                key: pc.id.to_string(),
                unique_id: comp.unique_id.to_string(),
                name: comp.name.clone(),
                version: comp.version.clone(),
                enabled: pc.enabled,
                dependencies,
            });
        }
        let versions: HashMap<String, String> = mods
            .iter()
            .map(|m| (m.key.clone(), m.version.clone()))
            .collect();
        let mut all = relations(&mods);
        let key = selected.id.to_string();
        let Some(mine) = all.remove(&key) else {
            return Ok(None);
        };
        let kind = |k: EdgeKind| match k {
            EdgeKind::Required => "required",
            EdgeKind::Optional => "optional",
            EdgeKind::ContentPackFor => "content_pack_for",
        };
        let name_of = |key: &str| names.get(key).map(|(n, _, _)| n.clone());

        let required_by: Vec<ModDependentDto> = mine
            .required_by
            .iter()
            .filter_map(|d| {
                let (name, unique_id, enabled) = names.get(&d.key)?.clone();
                Some(ModDependentDto {
                    profile_component_id: d.key.clone(),
                    name,
                    unique_id,
                    enabled,
                    minimum_version: d.minimum_version.clone(),
                    kind: kind(d.kind).to_string(),
                })
            })
            .collect();

        let (installed_reason, reason_detail) = match selected.installed_reason {
            InstalledReason::Direct => ("direct", "You installed this mod yourself.".to_string()),
            InstalledReason::Dependency => {
                let needing: Vec<String> = required_by
                    .iter()
                    .filter(|d| d.kind != "optional")
                    .map(|d| d.name.clone())
                    .collect();
                (
                    "dependency",
                    if needing.is_empty() {
                        "It was installed as a requirement of another mod, but nothing in this profile needs it any more.".to_string()
                    } else {
                        format!(
                            "It was installed because {} {} it.",
                            needing.join(", "),
                            if needing.len() == 1 { "needs" } else { "need" }
                        )
                    },
                )
            }
            InstalledReason::BundleCompanion => (
                "bundle_companion",
                "It came in the same download as another mod you installed.".to_string(),
            ),
        };

        Ok(Some(ModRelationsDto {
            installed_reason: installed_reason.to_string(),
            reason_detail,
            requires: mine
                .requires
                .iter()
                .map(|r| ModRequirementDto {
                    unique_id: r.unique_id.clone(),
                    name: r.target_key.as_deref().and_then(name_of),
                    installed_version: r.target_key.as_ref().and_then(|k| versions.get(k).cloned()),
                    minimum_version: r.minimum_version.clone(),
                    kind: kind(r.kind).to_string(),
                    status: match r.status {
                        EdgeStatus::Satisfied => "satisfied",
                        EdgeStatus::Missing => "missing",
                        EdgeStatus::Disabled => "disabled",
                        EdgeStatus::TooOld => "too_old",
                    }
                    .to_string(),
                })
                .collect(),
            required_by,
            broken_chains: mine
                .broken_chains
                .into_iter()
                .map(|(path, missing)| {
                    path.iter()
                        .map(|key| name_of(key).unwrap_or_else(|| key.clone()))
                        .chain(std::iter::once(missing))
                        .collect()
                })
                .collect(),
        }))
    }

    /// Every mod in the profile with a required dependency that is not met.
    pub fn profile_problems(
        &self,
        profile_id: &ProfileId,
    ) -> AppResult<Vec<crate::api::dto::ModProblemDto>> {
        let mut mods = Vec::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            let Some(comp) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            let mut dependencies: Vec<(String, Option<String>, EdgeKind)> = comp
                .manifest
                .dependencies
                .iter()
                .map(|d| {
                    (
                        d.unique_id.to_string(),
                        d.minimum_version.clone(),
                        if d.is_required {
                            EdgeKind::Required
                        } else {
                            EdgeKind::Optional
                        },
                    )
                })
                .collect();
            if let Some(host) = &comp.manifest.content_pack_for {
                dependencies.push((
                    host.unique_id.to_string(),
                    host.minimum_version.clone(),
                    EdgeKind::ContentPackFor,
                ));
            }
            mods.push(RelationMod {
                key: pc.id.to_string(),
                unique_id: comp.unique_id.to_string(),
                name: comp.name,
                version: comp.version,
                enabled: pc.enabled,
                dependencies,
            });
        }
        let mut problems: Vec<_> = relations(&mods)
            .into_iter()
            .filter_map(|(key, rel)| {
                let unmet: Vec<String> = rel
                    .requires
                    .into_iter()
                    .filter(|r| r.kind.is_required() && r.status != EdgeStatus::Satisfied)
                    .map(|r| r.unique_id)
                    .collect();
                (!unmet.is_empty()).then_some(crate::api::dto::ModProblemDto {
                    profile_component_id: key,
                    unmet_requirements: unmet,
                })
            })
            .collect();
        problems.sort_by(|a, b| a.profile_component_id.cmp(&b.profile_component_id));
        Ok(problems)
    }
}
