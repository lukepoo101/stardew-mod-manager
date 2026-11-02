use crate::dependency::graph::{
    DependencyEdge, DependencyEdgeType, DependencyGraph, DependencyNode,
};
use crate::dependency::relations::{EdgeKind, EdgeStatus};
use crate::ids::ModUniqueId;
use crate::manifest::model::Manifest;
use crate::version::SmapiVersion;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyFinding {
    pub unique_id: ModUniqueId,
    pub required_version: Option<String>,
    pub installed_version: Option<String>,
    pub is_required: bool,
    pub is_content_pack_framework: bool,
    pub satisfied: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyReport {
    pub is_installable: bool,
    pub smapi_compatible: bool,
    pub smapi_required_version: Option<String>,
    pub current_smapi_version: Option<String>,
    pub duplicate_id: bool,
    /// UniqueIDs held by more than one mod inside the package itself. SMAPI
    /// loads one mod per UniqueID, so such a package cannot be installed as
    /// it is; this is not the same as clashing with an installed mod.
    #[serde(default)]
    pub duplicate_in_package: Vec<ModUniqueId>,
    pub findings: Vec<DependencyFinding>,
}

/// SMAPI treats UniqueIDs case-insensitively.
fn same_id(a: &ModUniqueId, b: &ModUniqueId) -> bool {
    a.as_str().trim().eq_ignore_ascii_case(b.as_str().trim())
}

pub fn evaluate_dependencies(
    manifest: &Manifest,
    installed_manifests: &[(ModUniqueId, String)], // (UniqueID, version)
    current_smapi_version: Option<&str>,
) -> DependencyReport {
    let mut findings = Vec::new();
    let mut is_installable = true;

    // Check duplicate UniqueID
    let duplicate_id = installed_manifests
        .iter()
        .any(|(uid, _)| same_id(uid, &manifest.unique_id));

    if duplicate_id {
        is_installable = false;
    }

    // Check MinimumApiVersion against current SMAPI
    let mut smapi_compatible = true;
    if let Some(min_api_str) = manifest.minimum_api_version.as_deref() {
        if let Some(current_smapi_str) = current_smapi_version {
            if let (Ok(min_v), Ok(cur_v)) = (
                SmapiVersion::parse(min_api_str),
                SmapiVersion::parse(current_smapi_str),
            ) {
                if cur_v < min_v {
                    smapi_compatible = false;
                    is_installable = false;
                }
            }
        }
    }

    // Check ContentPackFor framework mod
    if let Some(ref cp) = manifest.content_pack_for {
        let matched = installed_manifests
            .iter()
            .find(|(uid, _)| same_id(uid, &cp.unique_id));

        match matched {
            None => {
                is_installable = false;
                findings.push(DependencyFinding {
                    unique_id: cp.unique_id.clone(),
                    required_version: cp.minimum_version.clone(),
                    installed_version: None,
                    is_required: true,
                    is_content_pack_framework: true,
                    satisfied: false,
                    reason: format!(
                        "Required content pack framework '{}' is not installed",
                        cp.unique_id
                    ),
                });
            }
            Some((_, inst_version)) => {
                let mut version_satisfied = true;
                let mut reason = format!("Framework '{}' is installed", cp.unique_id);

                if let Some(min_v_str) = cp.minimum_version.as_deref() {
                    if let (Ok(min_v), Ok(inst_v)) = (
                        SmapiVersion::parse(min_v_str),
                        SmapiVersion::parse(inst_version),
                    ) {
                        if inst_v < min_v {
                            version_satisfied = false;
                            is_installable = false;
                            reason = format!(
                                "Framework '{}' is installed ({}) but requires version >= {}",
                                cp.unique_id, inst_version, min_v_str
                            );
                        }
                    }
                }

                findings.push(DependencyFinding {
                    unique_id: cp.unique_id.clone(),
                    required_version: cp.minimum_version.clone(),
                    installed_version: Some(inst_version.clone()),
                    is_required: true,
                    is_content_pack_framework: true,
                    satisfied: version_satisfied,
                    reason,
                });
            }
        }
    }

    // Check Dependencies
    for dep in &manifest.dependencies {
        let matched = installed_manifests
            .iter()
            .find(|(uid, _)| same_id(uid, &dep.unique_id));

        match matched {
            None => {
                if dep.is_required {
                    is_installable = false;
                }
                findings.push(DependencyFinding {
                    unique_id: dep.unique_id.clone(),
                    required_version: dep.minimum_version.clone(),
                    installed_version: None,
                    is_required: dep.is_required,
                    is_content_pack_framework: false,
                    satisfied: !dep.is_required,
                    reason: if dep.is_required {
                        format!("Required dependency '{}' is missing", dep.unique_id)
                    } else {
                        format!("Optional dependency '{}' is not installed", dep.unique_id)
                    },
                });
            }
            Some((_, inst_version)) => {
                let mut version_satisfied = true;
                let mut reason = format!("Dependency '{}' is installed", dep.unique_id);

                if let Some(min_v_str) = dep.minimum_version.as_deref() {
                    if let (Ok(min_v), Ok(inst_v)) = (
                        SmapiVersion::parse(min_v_str),
                        SmapiVersion::parse(inst_version),
                    ) {
                        if inst_v < min_v {
                            version_satisfied = false;
                            if dep.is_required {
                                is_installable = false;
                            }
                            reason = format!(
                                "Dependency '{}' is installed ({}) but requires version >= {}",
                                dep.unique_id, inst_version, min_v_str
                            );
                        }
                    }
                }

                findings.push(DependencyFinding {
                    unique_id: dep.unique_id.clone(),
                    required_version: dep.minimum_version.clone(),
                    installed_version: Some(inst_version.clone()),
                    is_required: dep.is_required,
                    is_content_pack_framework: false,
                    satisfied: version_satisfied,
                    reason,
                });
            }
        }
    }

    DependencyReport {
        is_installable,
        smapi_compatible,
        smapi_required_version: manifest.minimum_api_version.clone(),
        current_smapi_version: current_smapi_version.map(|s| s.to_string()),
        duplicate_id,
        duplicate_in_package: Vec::new(),
        findings,
    }
}

pub fn evaluate_bundle_dependencies(
    bundle_manifests: &[Manifest],
    installed_manifests: &[(ModUniqueId, String)],
    current_smapi_version: Option<&str>,
) -> DependencyReport {
    let mut all_findings = Vec::new();
    let mut is_installable = true;
    let mut smapi_compatible = true;
    let mut duplicate_id = false;
    let mut duplicate_in_package: Vec<ModUniqueId> = Vec::new();
    let mut smapi_required_version: Option<(SmapiVersion, String)> = None;

    for (i, m) in bundle_manifests.iter().enumerate() {
        // The package's own mods come first: when it brings a newer copy of
        // an installed mod, that copy is what its other mods will load with.
        let mut available_manifests = Vec::new();
        for (j, other) in bundle_manifests.iter().enumerate() {
            if i == j {
                continue;
            }
            // Another mod in the package with the same UniqueID is a clash
            // inside the package, reported once below; it must not read as
            // "already installed in this profile".
            if same_id(&other.unique_id, &m.unique_id) {
                if j > i
                    && !duplicate_in_package
                        .iter()
                        .any(|id| same_id(id, &m.unique_id))
                {
                    duplicate_in_package.push(m.unique_id.clone());
                }
                continue;
            }
            available_manifests.push((other.unique_id.clone(), other.version.clone()));
        }
        available_manifests.extend_from_slice(installed_manifests);

        let report = evaluate_dependencies(m, &available_manifests, current_smapi_version);
        if !report.is_installable {
            is_installable = false;
        }
        if !report.smapi_compatible {
            smapi_compatible = false;
        }
        if report.duplicate_id {
            duplicate_id = true;
        }
        // The newest SMAPI any component asks for is what the package needs.
        if let Some(raw) = m.minimum_api_version.as_deref() {
            if let Ok(parsed) = SmapiVersion::parse(raw) {
                if smapi_required_version
                    .as_ref()
                    .is_none_or(|(highest, _)| parsed > *highest)
                {
                    smapi_required_version = Some((parsed, raw.to_string()));
                }
            }
        }
        all_findings.extend(report.findings);
    }
    if !duplicate_in_package.is_empty() {
        is_installable = false;
    }

    DependencyReport {
        is_installable,
        smapi_compatible,
        smapi_required_version: smapi_required_version.map(|(_, raw)| raw),
        current_smapi_version: current_smapi_version.map(|s| s.to_string()),
        duplicate_id,
        duplicate_in_package,
        findings: all_findings,
    }
}

/// A mod that could satisfy a requirement during an install review: one in
/// the profile, or one in the package being installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableMod {
    pub unique_id: ModUniqueId,
    pub name: String,
    pub version: String,
    pub enabled: bool,
}

/// One requirement of a mod under review, and whether what is available
/// meets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementCheck {
    pub unique_id: ModUniqueId,
    /// The available mod with that UniqueID, if any.
    pub name: Option<String>,
    pub installed_version: Option<String>,
    pub minimum_version: Option<String>,
    pub kind: EdgeKind,
    /// `TooOld` wins over `Disabled`: a disabled copy that is also too old
    /// would not help once enabled.
    pub status: EdgeStatus,
}

/// Every requirement a manifest declares (its content pack host first),
/// checked against `available`. The first available mod with a UniqueID is
/// the one used, so callers list the package's own mods before the profile's.
pub fn check_requirements(
    manifest: &Manifest,
    available: &[AvailableMod],
) -> Vec<RequirementCheck> {
    let host = manifest.content_pack_for.iter().map(|cp| {
        (
            &cp.unique_id,
            cp.minimum_version.as_deref(),
            EdgeKind::ContentPackFor,
        )
    });
    let dependencies = manifest.dependencies.iter().map(|d| {
        (
            &d.unique_id,
            d.minimum_version.as_deref(),
            if d.is_required {
                EdgeKind::Required
            } else {
                EdgeKind::Optional
            },
        )
    });
    host.chain(dependencies)
        .filter(|(id, _, _)| !same_id(id, &manifest.unique_id))
        .map(|(id, minimum, kind)| {
            let target = available.iter().find(|a| same_id(&a.unique_id, id));
            let status = match target {
                None => EdgeStatus::Missing,
                Some(target) => {
                    let too_old = minimum.is_some_and(|minimum| {
                        matches!(
                            (SmapiVersion::parse(minimum), SmapiVersion::parse(&target.version)),
                            (Ok(min), Ok(have)) if have < min
                        )
                    });
                    if too_old {
                        EdgeStatus::TooOld
                    } else if !target.enabled {
                        EdgeStatus::Disabled
                    } else {
                        EdgeStatus::Satisfied
                    }
                }
            };
            RequirementCheck {
                unique_id: id.clone(),
                name: target.map(|t| t.name.clone()),
                installed_version: target.map(|t| t.version.clone()),
                minimum_version: minimum.map(str::to_string),
                kind,
                status,
            }
        })
        .collect()
}

pub fn build_dependency_graph(
    manifests: &[Manifest],
    enabled_map: Option<&std::collections::HashMap<ModUniqueId, bool>>,
) -> DependencyGraph {
    let mut graph = DependencyGraph::new();

    for m in manifests {
        let enabled = enabled_map
            .and_then(|map| map.get(&m.unique_id).copied())
            .unwrap_or(true);

        graph.add_node(DependencyNode {
            unique_id: m.unique_id.clone(),
            version: m.version.clone(),
            name: m.name.clone(),
            enabled,
        });

        if let Some(ref cp) = m.content_pack_for {
            graph.add_edge(DependencyEdge {
                source_id: m.unique_id.clone(),
                target_id: cp.unique_id.clone(),
                minimum_version: cp.minimum_version.clone(),
                edge_type: DependencyEdgeType::ContentPackFor,
            });
        }

        for dep in &m.dependencies {
            let edge_type = if dep.is_required {
                DependencyEdgeType::Required
            } else {
                DependencyEdgeType::Optional
            };

            graph.add_edge(DependencyEdge {
                source_id: m.unique_id.clone(),
                target_id: dep.unique_id.clone(),
                minimum_version: dep.minimum_version.clone(),
                edge_type,
            });
        }
    }

    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ContentPackFor, Manifest};

    fn manifest(id: &str) -> Manifest {
        Manifest {
            unique_id: ModUniqueId::new(id),
            name: id.into(),
            author: "Author".into(),
            version: "1.0.0".into(),
            description: None,
            entry_dll: None,
            minimum_api_version: None,
            minimum_game_version: None,
            update_keys: Vec::new(),
            dependencies: Vec::new(),
            content_pack_for: None,
        }
    }

    #[test]
    fn unique_ids_match_regardless_of_case() {
        let installed = [(ModUniqueId::new("author.mod"), "0.9.0".to_string())];
        assert!(evaluate_dependencies(&manifest("Author.Mod"), &installed, None).duplicate_id);

        let mut pack = manifest("Me.Pack");
        pack.content_pack_for = Some(ContentPackFor {
            unique_id: ModUniqueId::new("AUTHOR.MOD"),
            minimum_version: None,
        });
        let report = evaluate_dependencies(&pack, &installed, None);
        assert!(
            report.findings.iter().all(|f| f.satisfied),
            "{:?}",
            report.findings
        );
    }

    #[test]
    fn a_host_in_the_same_archive_satisfies_its_content_pack() {
        let host = manifest("Pathoschild.ContentPatcher");
        let mut pack = manifest("Me.Pack");
        pack.content_pack_for = Some(ContentPackFor {
            unique_id: ModUniqueId::new("Pathoschild.ContentPatcher"),
            minimum_version: Some("1.0.0".into()),
        });
        let report = evaluate_bundle_dependencies(&[pack.clone(), host], &[], None);
        assert!(report.is_installable);
        assert!(report.findings.iter().all(|f| f.satisfied));

        let alone = evaluate_bundle_dependencies(&[pack], &[], None);
        assert!(!alone.is_installable);
    }

    #[test]
    fn two_mods_with_one_unique_id_in_a_package_are_a_clash_inside_it() {
        let report = evaluate_bundle_dependencies(
            &[
                manifest("Twin.Mod"),
                manifest("twin.mod"),
                manifest("Other.Mod"),
            ],
            &[],
            None,
        );
        assert!(!report.is_installable);
        assert!(!report.duplicate_id, "nothing is installed yet");
        assert_eq!(
            report.duplicate_in_package,
            vec![ModUniqueId::new("Twin.Mod")]
        );

        let installed = [(ModUniqueId::new("Other.Mod"), "1.0.0".to_string())];
        let report = evaluate_bundle_dependencies(
            &[manifest("A.Mod"), manifest("Other.Mod")],
            &installed,
            None,
        );
        assert!(report.duplicate_id);
        assert!(report.duplicate_in_package.is_empty());
    }

    #[test]
    fn a_bundle_reports_the_newest_smapi_any_component_needs() {
        let mut old = manifest("A.Old");
        old.minimum_api_version = Some("3.0.0".into());
        let mut new = manifest("A.New");
        new.minimum_api_version = Some("9.0.0".into());
        let report = evaluate_bundle_dependencies(&[old, new], &[], Some("4.1.10"));
        assert!(!report.smapi_compatible);
        assert_eq!(report.smapi_required_version.as_deref(), Some("9.0.0"));
    }

    fn available(id: &str, version: &str, enabled: bool) -> AvailableMod {
        AvailableMod {
            unique_id: ModUniqueId::new(id),
            name: format!("{id} name"),
            version: version.into(),
            enabled,
        }
    }

    #[test]
    fn requirements_say_missing_disabled_too_old_or_satisfied() {
        use crate::manifest::ModDependency;
        let mut fan = manifest("Ver.Fan");
        fan.content_pack_for = Some(ContentPackFor {
            unique_id: ModUniqueId::new("Host.Mod"),
            minimum_version: None,
        });
        let dep = |id: &str, min: Option<&str>, required: bool| ModDependency {
            unique_id: ModUniqueId::new(id),
            minimum_version: min.map(str::to_string),
            is_required: required,
        };
        fan.dependencies = vec![
            dep("Ver.Mod", None, true),
            dep("Core.Lib", Some("2.0.0"), true),
            dep("Gone.Mod", None, true),
            dep("Nice.ToHave", None, false),
            dep("Old.Off", Some("5.0"), true),
        ];
        let checks = check_requirements(
            &fan,
            &[
                available("host.mod", "1.0.0", true),
                available("Ver.Mod", "1.0.0", false),
                available("Core.Lib", "1.5.0", true),
                available("Old.Off", "1.0.0", false),
            ],
        );
        let status: Vec<_> = checks
            .iter()
            .map(|c| (c.unique_id.as_str().to_string(), c.kind, c.status))
            .collect();
        assert_eq!(
            status,
            vec![
                (
                    "Host.Mod".to_string(),
                    EdgeKind::ContentPackFor,
                    EdgeStatus::Satisfied
                ),
                (
                    "Ver.Mod".to_string(),
                    EdgeKind::Required,
                    EdgeStatus::Disabled
                ),
                (
                    "Core.Lib".to_string(),
                    EdgeKind::Required,
                    EdgeStatus::TooOld
                ),
                (
                    "Gone.Mod".to_string(),
                    EdgeKind::Required,
                    EdgeStatus::Missing
                ),
                (
                    "Nice.ToHave".to_string(),
                    EdgeKind::Optional,
                    EdgeStatus::Missing
                ),
                (
                    "Old.Off".to_string(),
                    EdgeKind::Required,
                    EdgeStatus::TooOld
                ),
            ]
        );
        assert_eq!(checks[2].installed_version.as_deref(), Some("1.5.0"));
        assert_eq!(checks[0].name.as_deref(), Some("host.mod name"));
    }
}
