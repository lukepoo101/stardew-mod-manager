//! Checks over the components that are live in a profile: required
//! dependencies and content-pack hosts, and UniqueIDs claimed more than once.
//!
//! SMAPI compares UniqueIDs case-insensitively, so these checks do too.

use crate::manifest::Manifest;
use crate::version::SmapiVersion;
use std::collections::BTreeMap;

/// One installed component of a profile.
#[derive(Debug, Clone)]
pub struct LiveComponent<'a> {
    pub manifest: &'a Manifest,
    pub enabled: bool,
    /// The component's folder inside the profile, for evidence.
    pub folder: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequirementProblem {
    /// Nothing in the profile has this UniqueID.
    Missing,
    /// Installed, but every copy is disabled.
    Disabled,
    /// Enabled, but older than the declared minimum.
    TooOld { installed: String },
    /// The declared minimum or installed version could not be read, so the
    /// requirement could not be checked.
    Unassessed { installed: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementFinding {
    /// The enabled mod that declares the requirement.
    pub dependent: String,
    pub dependent_id: String,
    /// The UniqueID it requires.
    pub required_id: String,
    pub minimum: Option<String>,
    /// The requirement is a `ContentPackFor` host rather than a dependency.
    pub host: bool,
    pub problem: RequirementProblem,
}

/// One copy of a UniqueID that is live more than once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCopy {
    pub name: String,
    pub version: String,
    pub folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateFinding {
    pub unique_id: String,
    pub copies: Vec<DuplicateCopy>,
}

fn key(id: &str) -> String {
    id.trim().to_lowercase()
}

/// Required dependencies and content-pack hosts of enabled components that
/// are missing, disabled, too old, or could not be checked. Optional
/// dependencies and disabled dependents are not reported.
pub fn check_requirements(components: &[LiveComponent<'_>]) -> Vec<RequirementFinding> {
    let mut by_id: BTreeMap<String, Vec<&LiveComponent<'_>>> = BTreeMap::new();
    for component in components {
        by_id
            .entry(key(component.manifest.unique_id.as_str()))
            .or_default()
            .push(component);
    }

    let mut findings = Vec::new();
    for component in components.iter().filter(|c| c.enabled) {
        let manifest = component.manifest;
        let host = manifest
            .content_pack_for
            .as_ref()
            .map(|cp| (cp.unique_id.as_str(), cp.minimum_version.as_deref(), true));
        let dependencies = manifest
            .dependencies
            .iter()
            .filter(|d| d.is_required)
            .map(|d| (d.unique_id.as_str(), d.minimum_version.as_deref(), false));
        for (required_id, minimum, is_host) in host.into_iter().chain(dependencies) {
            let problem = match by_id.get(&key(required_id)) {
                None => Some(RequirementProblem::Missing),
                Some(copies) => match copies.iter().find(|c| c.enabled) {
                    None => Some(RequirementProblem::Disabled),
                    Some(live) => minimum.and_then(|minimum| {
                        let installed = live.manifest.version.clone();
                        match (
                            SmapiVersion::parse(minimum),
                            SmapiVersion::parse(&installed),
                        ) {
                            (Ok(need), Ok(have)) if have < need => {
                                Some(RequirementProblem::TooOld { installed })
                            }
                            (Ok(_), Ok(_)) => None,
                            _ => Some(RequirementProblem::Unassessed { installed }),
                        }
                    }),
                },
            };
            if let Some(problem) = problem {
                findings.push(RequirementFinding {
                    dependent: manifest.name.clone(),
                    dependent_id: manifest.unique_id.to_string(),
                    required_id: required_id.to_string(),
                    minimum: minimum.map(str::to_string),
                    host: is_host,
                    problem,
                });
            }
        }
    }
    findings
}

/// UniqueIDs claimed by more than one enabled component, whatever their
/// folders are called. Disabled copies are not live and are not counted.
pub fn find_duplicate_ids(components: &[LiveComponent<'_>]) -> Vec<DuplicateFinding> {
    let mut by_id: BTreeMap<String, Vec<&LiveComponent<'_>>> = BTreeMap::new();
    for component in components.iter().filter(|c| c.enabled) {
        by_id
            .entry(key(component.manifest.unique_id.as_str()))
            .or_default()
            .push(component);
    }
    by_id
        .into_values()
        .filter(|copies| copies.len() > 1)
        .map(|copies| {
            let mut copies: Vec<DuplicateCopy> = copies
                .iter()
                .map(|c| DuplicateCopy {
                    name: c.manifest.name.clone(),
                    version: c.manifest.version.clone(),
                    folder: c.folder.to_string(),
                })
                .collect();
            copies.sort_by(|a, b| a.folder.cmp(&b.folder));
            DuplicateFinding {
                unique_id: copies_id(&copies, components),
                copies,
            }
        })
        .collect()
}

/// The UniqueID as written by the first copy (by folder), for display.
fn copies_id(copies: &[DuplicateCopy], components: &[LiveComponent<'_>]) -> String {
    copies
        .first()
        .and_then(|first| components.iter().find(|c| c.folder == first.folder))
        .map(|c| c.manifest.unique_id.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ModUniqueId;
    use crate::manifest::{ContentPackFor, ModDependency};

    fn manifest(id: &str, version: &str) -> Manifest {
        Manifest {
            unique_id: ModUniqueId::new(id),
            name: id.to_string(),
            author: "Author".to_string(),
            version: version.to_string(),
            description: None,
            entry_dll: None,
            minimum_api_version: None,
            minimum_game_version: None,
            update_keys: Vec::new(),
            dependencies: Vec::new(),
            content_pack_for: None,
        }
    }

    fn requires(mut m: Manifest, id: &str, min: Option<&str>, required: bool) -> Manifest {
        m.dependencies.push(ModDependency {
            unique_id: ModUniqueId::new(id),
            minimum_version: min.map(str::to_string),
            is_required: required,
        });
        m
    }

    fn pack_for(mut m: Manifest, id: &str, min: Option<&str>) -> Manifest {
        m.content_pack_for = Some(ContentPackFor {
            unique_id: ModUniqueId::new(id),
            minimum_version: min.map(str::to_string),
        });
        m
    }

    fn live<'a>(manifest: &'a Manifest, enabled: bool, folder: &'a str) -> LiveComponent<'a> {
        LiveComponent {
            manifest,
            enabled,
            folder,
        }
    }

    fn problems(findings: &[RequirementFinding]) -> Vec<(&str, &str, bool, &RequirementProblem)> {
        findings
            .iter()
            .map(|f| {
                (
                    f.dependent.as_str(),
                    f.required_id.as_str(),
                    f.host,
                    &f.problem,
                )
            })
            .collect()
    }

    #[test]
    fn missing_disabled_and_old_hosts_are_told_apart() {
        let missing = pack_for(manifest("Pack.Missing", "1.0"), "Frame.Absent", None);
        let disabled_host = manifest("Frame.Off", "2.0");
        let disabled = pack_for(manifest("Pack.Off", "1.0"), "Frame.Off", None);
        let old_host = manifest("Pathoschild.ContentPatcher", "1.9.0");
        let old = pack_for(
            manifest("Pack.Old", "1.0"),
            "pathoschild.contentpatcher",
            Some("2.0.0"),
        );
        let fine = pack_for(
            manifest("Pack.Fine", "1.0"),
            "Pathoschild.ContentPatcher",
            Some("1.5"),
        );
        let components = [
            live(&missing, true, "a"),
            live(&disabled_host, false, "b"),
            live(&disabled, true, "c"),
            live(&old_host, true, "d"),
            live(&old, true, "e"),
            live(&fine, true, "f"),
        ];
        assert_eq!(
            problems(&check_requirements(&components)),
            vec![
                (
                    "Pack.Missing",
                    "Frame.Absent",
                    true,
                    &RequirementProblem::Missing
                ),
                ("Pack.Off", "Frame.Off", true, &RequirementProblem::Disabled),
                (
                    "Pack.Old",
                    "pathoschild.contentpatcher",
                    true,
                    &RequirementProblem::TooOld {
                        installed: "1.9.0".to_string()
                    }
                ),
            ]
        );
    }

    #[test]
    fn disabled_dependents_and_optional_dependencies_are_quiet() {
        let off = requires(manifest("Off", "1.0"), "Absent", None, true);
        let optional = requires(manifest("Opt", "1.0"), "Absent", None, false);
        let components = [live(&off, false, "a"), live(&optional, true, "b")];
        assert!(check_requirements(&components).is_empty());
    }

    #[test]
    fn unreadable_minimums_are_unassessed_not_healthy() {
        let host = manifest("Lib", "1.0");
        let user = requires(manifest("User", "1.0"), "Lib", Some("one point two"), true);
        let components = [live(&host, true, "a"), live(&user, true, "b")];
        assert_eq!(
            problems(&check_requirements(&components)),
            vec![(
                "User",
                "Lib",
                false,
                &RequirementProblem::Unassessed {
                    installed: "1.0".to_string()
                }
            )]
        );
    }

    #[test]
    fn duplicates_ignore_case_folders_and_disabled_copies() {
        let one = manifest("Author.Mod", "1.0");
        let two = manifest("author.mod", "1.1");
        let off = manifest("Author.Mod", "0.9");
        let other = manifest("Author.Other", "1.0");
        let components = [
            live(&one, true, "Mod"),
            live(&two, true, "Mod (copy)"),
            live(&off, false, "Old"),
            live(&other, true, "Other"),
        ];
        let duplicates = find_duplicate_ids(&components);
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].unique_id, "Author.Mod");
        assert_eq!(
            duplicates[0]
                .copies
                .iter()
                .map(|c| (c.folder.as_str(), c.version.as_str()))
                .collect::<Vec<_>>(),
            vec![("Mod", "1.0"), ("Mod (copy)", "1.1")]
        );

        let single = [live(&one, true, "Mod"), live(&off, false, "Old")];
        assert!(find_duplicate_ids(&single).is_empty());
    }
}
