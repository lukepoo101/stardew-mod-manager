use crate::dependency::graph::{
    DependencyEdge, DependencyEdgeType, DependencyGraph, DependencyNode,
};
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
    pub findings: Vec<DependencyFinding>,
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
        .any(|(uid, _)| uid == &manifest.unique_id);

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
            .find(|(uid, _)| uid == &cp.unique_id);

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
            .find(|(uid, _)| uid == &dep.unique_id);

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

    for (i, m) in bundle_manifests.iter().enumerate() {
        let mut available_manifests = installed_manifests.to_vec();
        for (j, other) in bundle_manifests.iter().enumerate() {
            if i != j {
                available_manifests.push((other.unique_id.clone(), other.version.clone()));
            }
        }

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
        all_findings.extend(report.findings);
    }

    DependencyReport {
        is_installable,
        smapi_compatible,
        smapi_required_version: None,
        current_smapi_version: current_smapi_version.map(|s| s.to_string()),
        duplicate_id,
        findings: all_findings,
    }
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
