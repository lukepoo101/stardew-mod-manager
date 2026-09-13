use crate::domain::{InstalledMod, Manifest};
use crate::manifest::version::SmapiVersion;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyFinding {
    pub unique_id: String,
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
    installed_mods: &[InstalledMod],
    current_smapi_version: Option<&str>,
) -> DependencyReport {
    let mut findings = Vec::new();
    let mut is_installable = true;

    // Check duplicate UniqueID
    let duplicate_id = installed_mods
        .iter()
        .any(|m| m.unique_id.eq_ignore_ascii_case(&manifest.unique_id));

    if duplicate_id {
        is_installable = false;
    }

    // Check MinimumApiVersion against current SMAPI
    let mut smapi_compatible = true;
    if let Some(ref min_api_str) = manifest.minimum_api_version {
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
        let matched = installed_mods
            .iter()
            .find(|m| m.unique_id.eq_ignore_ascii_case(&cp.unique_id));

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
            Some(installed) => {
                let mut version_satisfied = true;
                let mut reason = format!("Framework '{}' is installed", cp.unique_id);

                if let Some(ref min_v_str) = cp.minimum_version {
                    if let (Ok(min_v), Ok(inst_v)) = (
                        SmapiVersion::parse(min_v_str),
                        SmapiVersion::parse(&installed.version),
                    ) {
                        if inst_v < min_v {
                            version_satisfied = false;
                            is_installable = false;
                            reason = format!(
                                "Framework '{}' is installed ({}) but requires version >= {}",
                                cp.unique_id, installed.version, min_v_str
                            );
                        }
                    }
                }

                findings.push(DependencyFinding {
                    unique_id: cp.unique_id.clone(),
                    required_version: cp.minimum_version.clone(),
                    installed_version: Some(installed.version.clone()),
                    is_required: true,
                    is_content_pack_framework: true,
                    satisfied: version_satisfied,
                    reason,
                });
            }
        }
    }

    // Check declared Dependencies
    for dep in &manifest.dependencies {
        let matched = installed_mods
            .iter()
            .find(|m| m.unique_id.eq_ignore_ascii_case(&dep.unique_id));

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
                        format!("Required mod '{}' is not installed", dep.unique_id)
                    } else {
                        format!("Optional mod '{}' is not installed", dep.unique_id)
                    },
                });
            }
            Some(installed) => {
                let mut version_satisfied = true;
                let mut reason = format!(
                    "Mod '{}' is installed ({})",
                    dep.unique_id, installed.version
                );

                if let Some(ref min_v_str) = dep.minimum_version {
                    if let (Ok(min_v), Ok(inst_v)) = (
                        SmapiVersion::parse(min_v_str),
                        SmapiVersion::parse(&installed.version),
                    ) {
                        if inst_v < min_v {
                            version_satisfied = false;
                            if dep.is_required {
                                is_installable = false;
                            }
                            reason = format!(
                                "Mod '{}' is installed ({}) but requires version >= {}",
                                dep.unique_id, installed.version, min_v_str
                            );
                        }
                    }
                }

                findings.push(DependencyFinding {
                    unique_id: dep.unique_id.clone(),
                    required_version: dep.minimum_version.clone(),
                    installed_version: Some(installed.version.clone()),
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
    manifests: &[Manifest],
    installed_mods: &[InstalledMod],
    current_smapi_version: Option<&str>,
) -> DependencyReport {
    let mut report = DependencyReport {
        is_installable: true,
        smapi_compatible: true,
        smapi_required_version: None,
        current_smapi_version: current_smapi_version.map(str::to_owned),
        duplicate_id: false,
        findings: Vec::new(),
    };
    let mut ids = std::collections::HashSet::new();
    for manifest in manifests {
        if !ids.insert(manifest.unique_id.to_lowercase())
            || installed_mods
                .iter()
                .any(|m| m.unique_id.eq_ignore_ascii_case(&manifest.unique_id))
        {
            report.duplicate_id = true;
            report.is_installable = false;
        }
        let mut available = installed_mods.to_vec();
        for other in manifests
            .iter()
            .filter(|m| !m.unique_id.eq_ignore_ascii_case(&manifest.unique_id))
        {
            available.push(InstalledMod {
                id: String::new(),
                setup_id: String::new(),
                package_id: String::new(),
                unique_id: other.unique_id.clone(),
                name: other.name.clone(),
                author: other.author.clone(),
                version: other.version.clone(),
                description: None,
                raw_manifest: String::new(),
                relative_target_path: String::new(),
                file_inventory: Vec::new(),
                installed_at: chrono::Utc::now(),
            });
        }
        let result = evaluate_dependencies(manifest, &available, current_smapi_version);
        report.is_installable &= result.is_installable;
        report.smapi_compatible &= result.smapi_compatible;
        report.duplicate_id |= result.duplicate_id;
        if result.smapi_required_version.is_some() {
            report.smapi_required_version = result.smapi_required_version;
        }
        report.findings.extend(result.findings);
    }
    report
}
