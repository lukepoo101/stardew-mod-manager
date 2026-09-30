//! Who needs what in a profile, and the chains behind unmet requirements.
//!
//! UniqueIDs are matched case-insensitively, as SMAPI matches them. Only
//! enabled mods can satisfy a requirement, because a disabled mod is not
//! loaded. Optional dependencies are reported but never make a chain broken.

use crate::version::SmapiVersion;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Required,
    Optional,
    /// The framework a content pack is for; always required.
    ContentPackFor,
}

impl EdgeKind {
    pub fn is_required(self) -> bool {
        !matches!(self, EdgeKind::Optional)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStatus {
    Satisfied,
    /// No mod with that UniqueID is in the profile.
    Missing,
    /// It is in the profile but disabled.
    Disabled,
    /// It is enabled but older than the minimum version.
    TooOld,
}

#[derive(Debug, Clone)]
pub struct RelationMod {
    pub key: String,
    pub unique_id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub dependencies: Vec<(String, Option<String>, EdgeKind)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub unique_id: String,
    /// The mod in the profile with that UniqueID, if any.
    pub target_key: Option<String>,
    pub minimum_version: Option<String>,
    pub kind: EdgeKind,
    pub status: EdgeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependent {
    pub key: String,
    pub minimum_version: Option<String>,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relations {
    pub requires: Vec<Requirement>,
    /// Mods in the profile that depend on this one.
    pub required_by: Vec<Dependent>,
    /// Paths of required edges from this mod to an unmet requirement, as mod
    /// keys followed by the UniqueID that is not satisfied. A direct unmet
    /// requirement is a path of one key.
    pub broken_chains: Vec<(Vec<String>, String)>,
}

const MAX_CHAINS: usize = 20;

fn status(target: Option<&RelationMod>, minimum: Option<&str>) -> EdgeStatus {
    let Some(target) = target else {
        return EdgeStatus::Missing;
    };
    if !target.enabled {
        return EdgeStatus::Disabled;
    }
    if let Some(minimum) = minimum {
        if let (Ok(min), Ok(have)) = (
            SmapiVersion::parse(minimum),
            SmapiVersion::parse(&target.version),
        ) {
            if have < min {
                return EdgeStatus::TooOld;
            }
        }
    }
    EdgeStatus::Satisfied
}

/// Relations for every mod, keyed by its key.
pub fn relations(mods: &[RelationMod]) -> HashMap<String, Relations> {
    // With duplicate UniqueIDs, prefer an enabled copy, as SMAPI would load it.
    let mut by_id: HashMap<String, &RelationMod> = HashMap::new();
    for m in mods {
        let id = m.unique_id.to_lowercase();
        match by_id.get(&id) {
            Some(existing) if existing.enabled || !m.enabled => {}
            _ => {
                by_id.insert(id, m);
            }
        }
    }
    let by_key: HashMap<&str, &RelationMod> = mods.iter().map(|m| (m.key.as_str(), m)).collect();

    let requires_of = |m: &RelationMod| -> Vec<Requirement> {
        m.dependencies
            .iter()
            .map(|(id, minimum, kind)| {
                let target = by_id.get(&id.to_lowercase()).copied();
                Requirement {
                    unique_id: id.clone(),
                    target_key: target.map(|t| t.key.clone()),
                    minimum_version: minimum.clone(),
                    kind: *kind,
                    status: status(target, minimum.as_deref()),
                }
            })
            .collect()
    };
    let all_requires: HashMap<&str, Vec<Requirement>> = mods
        .iter()
        .map(|m| (m.key.as_str(), requires_of(m)))
        .collect();

    let mut required_by: HashMap<&str, Vec<Dependent>> = HashMap::new();
    for m in mods {
        for requirement in &all_requires[m.key.as_str()] {
            if let Some(target) = &requirement.target_key {
                if target != &m.key {
                    required_by
                        .entry(by_key[target.as_str()].key.as_str())
                        .or_default()
                        .push(Dependent {
                            key: m.key.clone(),
                            minimum_version: requirement.minimum_version.clone(),
                            kind: requirement.kind,
                        });
                }
            }
        }
    }

    mods.iter()
        .map(|m| {
            let mut chains = Vec::new();
            let mut path = vec![m.key.clone()];
            let mut on_path: HashSet<String> = HashSet::from([m.key.clone()]);
            walk(&all_requires, &mut path, &mut on_path, &mut chains);
            let mut dependents = required_by.remove(m.key.as_str()).unwrap_or_default();
            dependents.sort_by(|a, b| a.key.cmp(&b.key));
            (
                m.key.clone(),
                Relations {
                    requires: all_requires[m.key.as_str()].clone(),
                    required_by: dependents,
                    broken_chains: chains,
                },
            )
        })
        .collect()
}

/// Depth-first over required edges. `on_path` stops cycles, and each unmet
/// requirement is reported once per path that reaches it.
fn walk(
    requires: &HashMap<&str, Vec<Requirement>>,
    path: &mut Vec<String>,
    on_path: &mut HashSet<String>,
    chains: &mut Vec<(Vec<String>, String)>,
) {
    let current = path.last().cloned().unwrap_or_default();
    for requirement in requires.get(current.as_str()).into_iter().flatten() {
        if chains.len() >= MAX_CHAINS {
            return;
        }
        if !requirement.kind.is_required() {
            continue;
        }
        if requirement.status != EdgeStatus::Satisfied {
            chains.push((path.clone(), requirement.unique_id.clone()));
            continue;
        }
        let Some(next) = &requirement.target_key else {
            continue;
        };
        if on_path.insert(next.clone()) {
            path.push(next.clone());
            walk(requires, path, on_path, chains);
            path.pop();
            on_path.remove(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(
        key: &str,
        version: &str,
        enabled: bool,
        deps: &[(&str, Option<&str>, EdgeKind)],
    ) -> RelationMod {
        RelationMod {
            key: key.to_string(),
            unique_id: format!("Mod.{key}"),
            name: key.to_string(),
            version: version.to_string(),
            enabled,
            dependencies: deps
                .iter()
                .map(|(id, min, kind)| (id.to_string(), min.map(str::to_string), *kind))
                .collect(),
        }
    }
    use EdgeKind::*;

    #[test]
    fn statuses_distinguish_missing_disabled_and_too_old() {
        let mods = [
            m(
                "A",
                "1.0",
                true,
                &[
                    ("mod.b", Some("2.0"), Required),
                    ("Mod.C", None, Required),
                    ("Mod.Gone", None, Required),
                    ("Mod.Opt", None, Optional),
                ],
            ),
            m("B", "1.5", true, &[]),
            m("C", "1.0", false, &[]),
        ];
        let rel = relations(&mods);
        let statuses: Vec<_> = rel["A"].requires.iter().map(|r| r.status).collect();
        assert_eq!(
            statuses,
            vec![
                EdgeStatus::TooOld,
                EdgeStatus::Disabled,
                EdgeStatus::Missing,
                EdgeStatus::Missing
            ]
        );
        // The optional edge is reported but is not a broken chain.
        assert_eq!(rel["A"].broken_chains.len(), 3);
        assert_eq!(
            rel["B"].required_by,
            vec![Dependent {
                key: "A".into(),
                minimum_version: Some("2.0".into()),
                kind: Required,
            }]
        );
    }

    #[test]
    fn a_transitive_gap_is_traced_back_to_every_top_level_mod() {
        let mods = [
            m("Top1", "1", true, &[("Mod.Mid", None, Required)]),
            m("Top2", "1", true, &[("Mod.Mid", None, ContentPackFor)]),
            m("Mid", "1", true, &[("Mod.Leaf", None, Required)]),
        ];
        let rel = relations(&mods);
        assert_eq!(
            rel["Top1"].broken_chains,
            vec![(
                vec!["Top1".to_string(), "Mid".to_string()],
                "Mod.Leaf".to_string()
            )]
        );
        assert_eq!(rel["Top2"].broken_chains[0].0, vec!["Top2", "Mid"]);
        assert_eq!(rel["Mid"].required_by.len(), 2);
    }

    #[test]
    fn cycles_terminate() {
        let mods = [
            m("A", "1", true, &[("Mod.B", None, Required)]),
            m(
                "B",
                "1",
                true,
                &[("Mod.A", None, Required), ("Mod.X", None, Required)],
            ),
        ];
        let rel = relations(&mods);
        assert_eq!(
            rel["A"].broken_chains,
            vec![(vec!["A".to_string(), "B".to_string()], "Mod.X".to_string())]
        );
    }

    #[test]
    fn an_enabled_copy_wins_over_a_disabled_duplicate() {
        let mut disabled = m("Old", "1", false, &[]);
        disabled.unique_id = "Mod.Dup".into();
        let mut enabled = m("New", "2", true, &[]);
        enabled.unique_id = "Mod.Dup".into();
        let mods = [
            disabled,
            enabled,
            m("User", "1", true, &[("Mod.Dup", None, Required)]),
        ];
        let rel = relations(&mods);
        assert_eq!(rel["User"].requires[0].status, EdgeStatus::Satisfied);
        assert_eq!(rel["User"].requires[0].target_key.as_deref(), Some("New"));
    }
}
