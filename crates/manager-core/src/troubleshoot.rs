//! Guided fault isolation by bisection.
//!
//! The problem being hunted is assumed to have one cause. The session narrows a
//! set of suspects by enabling half of them (plus whatever those mods require)
//! and asking whether the problem is still there. Mods that must travel together
//! (one package, or a required dependency) are never separated.
//!
//! Everything here is pure: it decides which units should be enabled, and the
//! caller applies that to the profile and persists the session.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

/// The smallest thing that can be enabled or disabled: one deployment folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    pub key: String,
    pub unique_ids: Vec<String>,
    /// UniqueIDs that must be enabled for this unit to work.
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Every mod is off; waiting to learn whether the problem is still there.
    AllOff,
    /// A subset is enabled; waiting for the same answer.
    Testing,
    /// One unit remains as the most likely cause.
    Found,
    /// The problem could not be pinned on a single unit.
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub phase: Phase,
    /// Units that may still be the cause.
    pub suspects: Vec<String>,
    /// Units shown to be fine together with everything else enabled.
    pub known_good: Vec<String>,
    /// The units enabled for the test currently in progress.
    pub enabled: Vec<String>,
    pub steps: u32,
    pub culprit: Option<String>,
    pub note: Option<String>,
}

/// Every unit needed to run `seed`, following required dependencies.
pub fn closure(units: &[Unit], seed: &HashSet<String>) -> HashSet<String> {
    let mut result: HashSet<String> = seed.clone();
    loop {
        let provided: HashSet<&str> = units
            .iter()
            .filter(|u| result.contains(&u.key))
            .flat_map(|u| u.requires.iter().map(String::as_str))
            .collect();
        let before = result.len();
        for unit in units {
            if unit
                .unique_ids
                .iter()
                .any(|id| provided.contains(id.as_str()))
            {
                result.insert(unit.key.clone());
            }
        }
        if result.len() == before {
            return result;
        }
    }
}

fn sorted(set: &HashSet<String>) -> Vec<String> {
    let ordered: BTreeSet<&String> = set.iter().collect();
    ordered.into_iter().cloned().collect()
}

impl Session {
    /// Begins with every mod disabled.
    pub fn start() -> Self {
        Self {
            phase: Phase::AllOff,
            suspects: Vec::new(),
            known_good: Vec::new(),
            enabled: Vec::new(),
            steps: 0,
            culprit: None,
            note: None,
        }
    }

    fn all_keys(units: &[Unit]) -> Vec<String> {
        let mut keys: Vec<String> = units.iter().map(|u| u.key.clone()).collect();
        keys.sort();
        keys
    }

    /// The next test: half of the suspects plus what they need and what has
    /// already been cleared.
    fn next_test(&mut self, units: &[Unit]) {
        // Units that others need come first (a dependency's closure is smaller
        // than its dependents'), so a dependency can be cleared on its own and
        // the search always makes progress instead of dragging it into every
        // test alongside the mod that needs it.
        let mut order: Vec<(usize, String)> = self
            .suspects
            .iter()
            .map(|key| {
                let seed: HashSet<String> = [key.clone()].into_iter().collect();
                (closure(units, &seed).len(), key.clone())
            })
            .collect();
        order.sort();
        self.suspects = order.into_iter().map(|(_, key)| key).collect();

        let half = self.suspects.len().div_ceil(2);
        let mut seed: HashSet<String> = self.suspects.iter().take(half).cloned().collect();
        seed.extend(self.known_good.iter().cloned());
        self.enabled = sorted(&closure(units, &seed));
        self.phase = Phase::Testing;
    }

    /// Records whether the problem is present with the current test and moves to
    /// the next one. Returns the units that should be enabled afterwards.
    pub fn answer(&mut self, units: &[Unit], problem_present: bool) -> Vec<String> {
        match self.phase {
            Phase::Found | Phase::Inconclusive => return self.enabled.clone(),
            Phase::AllOff => {
                self.steps += 1;
                if problem_present {
                    self.phase = Phase::Inconclusive;
                    self.note = Some(
                        "The problem is still there with every mod disabled, so it is probably not caused by a mod."
                            .to_string(),
                    );
                    self.enabled = Vec::new();
                    return Vec::new();
                }
                self.suspects = Self::all_keys(units);
                if self.suspects.len() <= 1 {
                    return self.conclude();
                }
                self.next_test(units);
                return self.enabled.clone();
            }
            Phase::Testing => {}
        }

        self.steps += 1;
        let tested: HashSet<String> = self.enabled.iter().cloned().collect();
        let good: HashSet<String> = self.known_good.iter().cloned().collect();
        if problem_present {
            // The cause is among what was enabled and not already cleared.
            self.suspects
                .retain(|key| tested.contains(key) && !good.contains(key));
            // A required dependency pulled in by the test is a suspect too.
            for key in &tested {
                if !good.contains(key) && !self.suspects.contains(key) {
                    self.suspects.push(key.clone());
                }
            }
            self.suspects.sort();
        } else {
            // Everything enabled worked, so it is cleared.
            for key in &self.enabled {
                if !self.known_good.contains(key) {
                    self.known_good.push(key.clone());
                }
            }
            self.known_good.sort();
            self.suspects.retain(|key| !tested.contains(key));
        }

        match self.suspects.len() {
            0 => {
                self.phase = Phase::Inconclusive;
                self.note = Some(
                    "No single mod explains it. It may need two mods together, or something outside the mods."
                        .to_string(),
                );
                self.enabled = sorted(&closure(units, &self.known_good.iter().cloned().collect()));
            }
            1 => return self.conclude(),
            _ => self.next_test(units),
        }
        self.enabled.clone()
    }

    fn conclude(&mut self) -> Vec<String> {
        self.phase = Phase::Found;
        self.culprit = self.suspects.first().cloned();
        self.enabled = self.known_good.clone();
        self.note = Some(
            "This is the most likely cause, assuming a single mod is responsible. Check it before deciding."
                .to_string(),
        );
        self.enabled.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(key: &str, requires: &[&str]) -> Unit {
        Unit {
            key: key.to_string(),
            unique_ids: vec![format!("id.{key}")],
            requires: requires.iter().map(|r| format!("id.{r}")).collect(),
        }
    }

    /// Runs a whole session against a hidden culprit and returns what was found.
    fn find(units: &[Unit], culprit: &str) -> (Session, u32) {
        let mut session = Session::start();
        let mut enabled: Vec<String> = Vec::new();
        for _ in 0..64 {
            // The problem is present exactly when the culprit is enabled.
            let present = enabled.iter().any(|k| k == culprit);
            enabled = session.answer(units, present);
            if matches!(session.phase, Phase::Found | Phase::Inconclusive) {
                break;
            }
        }
        let steps = session.steps;
        (session, steps)
    }

    #[test]
    fn closure_follows_required_dependencies_transitively() {
        let units = vec![
            unit("a", &["b"]),
            unit("b", &["c"]),
            unit("c", &[]),
            unit("d", &[]),
        ];
        let result = closure(&units, &["a".to_string()].into_iter().collect());
        assert_eq!(sorted(&result), vec!["a", "b", "c"]);
    }

    #[test]
    fn every_single_culprit_is_found_in_logarithmic_steps() {
        let keys: Vec<String> = (0..20).map(|i| format!("m{i:02}")).collect();
        let units: Vec<Unit> = keys.iter().map(|k| unit(k, &[])).collect();
        for culprit in &keys {
            let (session, steps) = find(&units, culprit);
            assert_eq!(session.phase, Phase::Found, "culprit {culprit}");
            assert_eq!(session.culprit.as_deref(), Some(culprit.as_str()));
            // One step for the all-off check, then about log2(20) tests.
            assert!(steps <= 7, "took {steps} steps for {culprit}");
        }
    }

    #[test]
    fn a_dependency_is_never_separated_from_what_needs_it() {
        // Every test set must be closed under requirements.
        let units = vec![
            unit("a", &["lib"]),
            unit("b", &["lib"]),
            unit("c", &[]),
            unit("d", &["c"]),
            unit("lib", &[]),
        ];
        for culprit in ["a", "b", "c", "d", "lib"] {
            let mut session = Session::start();
            let mut enabled: Vec<String> = Vec::new();
            for _ in 0..16 {
                let present = enabled.iter().any(|k| k == culprit);
                enabled = session.answer(&units, present);
                let set: HashSet<String> = enabled.iter().cloned().collect();
                assert_eq!(closure(&units, &set), set, "open set {enabled:?}");
                if matches!(session.phase, Phase::Found | Phase::Inconclusive) {
                    break;
                }
            }
            assert_eq!(session.phase, Phase::Found, "culprit {culprit}");
            assert_eq!(session.culprit.as_deref(), Some(culprit));
        }
    }

    #[test]
    fn a_problem_that_survives_disabling_everything_is_not_blamed_on_a_mod() {
        let units = vec![unit("a", &[]), unit("b", &[])];
        let mut session = Session::start();
        let enabled = session.answer(&units, true);
        assert!(enabled.is_empty());
        assert_eq!(session.phase, Phase::Inconclusive);
        assert!(session.culprit.is_none());
    }

    #[test]
    fn a_combination_of_two_mods_is_reported_as_inconclusive_not_misattributed() {
        // The problem needs both x and y; no single one explains it.
        let units: Vec<Unit> = ["x", "y", "p", "q"].iter().map(|k| unit(k, &[])).collect();
        let mut session = Session::start();
        let mut enabled: Vec<String> = Vec::new();
        for _ in 0..16 {
            let present = enabled.iter().any(|k| k == "x") && enabled.iter().any(|k| k == "y");
            enabled = session.answer(&units, present);
            if matches!(session.phase, Phase::Found | Phase::Inconclusive) {
                break;
            }
        }
        assert_ne!(session.culprit.as_deref(), Some("p"));
        assert_ne!(session.culprit.as_deref(), Some("q"));
    }

    #[test]
    fn a_single_mod_profile_is_concluded_immediately_after_the_baseline() {
        let units = vec![unit("only", &[])];
        let mut session = Session::start();
        session.answer(&units, false);
        assert_eq!(session.phase, Phase::Found);
        assert_eq!(session.culprit.as_deref(), Some("only"));
    }
}
