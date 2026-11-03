//! Guided fault isolation by bisection.
//!
//! The session narrows a set of suspects by enabling half of them (plus
//! whatever those mods require) and asking whether the problem is still there.
//! Mods that must travel together (one package, or a required dependency) are
//! never separated.
//!
//! Bisection can only blame a single mod. When one suspect is left it is
//! tested on its own, without the mods already cleared, before it is named:
//! a problem that needs two mods together disappears then, and the session
//! ends inconclusive, naming the mod involved and the ones it was on with,
//! rather than blaming one of them. The user can then ask to find the other
//! mod: the involved mod stays on and the candidates are halved the same way,
//! about log2 of them more tests, and the pair is checked on its own before
//! it is named. If the pair alone does not show it, three or more mods are
//! needed and the session says so.
//!
//! UniqueIDs are compared ignoring case, as SMAPI does. Mods whose
//! requirements loop back on each other can only be tested together, so
//! [`collapse_cycles`] makes each loop one unit before a session sees it.
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
    /// The last suspect is enabled on its own (with only what it requires),
    /// to check that it causes the problem without any other mod.
    Confirming,
    /// One unit remains as the most likely cause.
    Found,
    /// The problem could not be pinned on a single unit.
    Inconclusive,
    /// Looking for the other mod of a pair: the involved mod is on with half
    /// of the candidates.
    PartnerSearch,
    /// The involved mod and the last candidate are on by themselves.
    PartnerConfirm,
    /// Two units together cause it: `involved` and `partner`.
    FoundPair,
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
    /// The units enabled the most recent time the problem happened.
    #[serde(default)]
    pub last_present: Option<Vec<String>>,
    /// When inconclusive because no mod causes it alone: the suspect that
    /// does cause it, but only together with one of `partners`.
    #[serde(default)]
    pub involved: Option<String>,
    #[serde(default)]
    pub partners: Vec<String>,
    /// While looking for the other mod: the candidates left.
    #[serde(default)]
    pub partner_suspects: Vec<String>,
    /// The other mod of a pair, once found.
    #[serde(default)]
    pub partner: Option<String>,
}

/// What joins the keys of units merged by [`collapse_cycles`]. Unit keys are
/// deployment ids (UUIDs), which never contain it.
pub const GROUP_SEPARATOR: char = '+';

/// The original unit keys a (possibly merged) unit key stands for.
pub fn group_members(key: &str) -> impl Iterator<Item = &str> {
    key.split(GROUP_SEPARATOR)
}

fn id_key(id: &str) -> String {
    id.trim().to_lowercase()
}

/// Every unit needed to run `seed`, following required dependencies.
/// UniqueIDs match ignoring case.
pub fn closure(units: &[Unit], seed: &HashSet<String>) -> HashSet<String> {
    let mut result: HashSet<String> = seed.clone();
    loop {
        let provided: HashSet<String> = units
            .iter()
            .filter(|u| result.contains(&u.key))
            .flat_map(|u| u.requires.iter().map(|id| id_key(id)))
            .collect();
        let before = result.len();
        for unit in units {
            if unit
                .unique_ids
                .iter()
                .any(|id| provided.contains(&id_key(id)))
            {
                result.insert(unit.key.clone());
            }
        }
        if result.len() == before {
            return result;
        }
    }
}

/// Merges every set of units whose requirements loop back on each other
/// into one unit, keyed by its members' keys joined with
/// [`GROUP_SEPARATOR`]. Bisection cannot split such a set (each needs the
/// others), and asking it to would repeat the same test forever.
pub fn collapse_cycles(units: &[Unit]) -> Vec<Unit> {
    let reach: Vec<HashSet<String>> = units
        .iter()
        .map(|u| closure(units, &[u.key.clone()].into_iter().collect()))
        .collect();
    let mut placed: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (index, unit) in units.iter().enumerate() {
        if placed.contains(&unit.key) {
            continue;
        }
        let mut members: Vec<usize> = units
            .iter()
            .enumerate()
            .filter(|(other, candidate)| {
                *other == index
                    || (reach[index].contains(&candidate.key) && reach[*other].contains(&unit.key))
            })
            .map(|(other, _)| other)
            .collect();
        members.sort_by(|a, b| units[*a].key.cmp(&units[*b].key));
        for member in &members {
            placed.insert(units[*member].key.clone());
        }
        if members.len() == 1 {
            out.push(unit.clone());
            continue;
        }
        let own: HashSet<String> = members
            .iter()
            .flat_map(|m| units[*m].unique_ids.iter().map(|id| id_key(id)))
            .collect();
        out.push(Unit {
            key: members
                .iter()
                .map(|m| units[*m].key.as_str())
                .collect::<Vec<_>>()
                .join(&GROUP_SEPARATOR.to_string()),
            unique_ids: members
                .iter()
                .flat_map(|m| units[*m].unique_ids.iter().cloned())
                .collect(),
            requires: members
                .iter()
                .flat_map(|m| units[*m].requires.iter().cloned())
                .filter(|id| !own.contains(&id_key(id)))
                .collect(),
        });
    }
    out
}

/// Units in `order` (ascending closure size), and the half to test: whole
/// units from the front until half the candidates are covered.
fn by_closure_size(units: &[Unit], keys: &[String]) -> Vec<String> {
    let mut order: Vec<(usize, String)> = keys
        .iter()
        .map(|key| {
            let seed: HashSet<String> = [key.clone()].into_iter().collect();
            (closure(units, &seed).len(), key.clone())
        })
        .collect();
    order.sort();
    order.into_iter().map(|(_, key)| key).collect()
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
            last_present: None,
            involved: None,
            partners: Vec::new(),
            partner_suspects: Vec::new(),
            partner: None,
        }
    }

    /// Whether the other mod of an inconclusive pair can be looked for.
    pub fn can_narrow(&self) -> bool {
        self.phase == Phase::Inconclusive && self.involved.is_some() && !self.partners.is_empty()
    }

    /// About how many more tests finding the other mod takes: halving the
    /// candidates, then one test of the pair on its own.
    pub fn narrow_estimate(&self) -> u32 {
        let candidates = self.partners.len().max(1) as u32;
        u32::BITS - (candidates - 1).leading_zeros() + 1
    }

    /// Starts looking for the other mod of an inconclusive pair. Returns the
    /// units to enable; does nothing unless [`Self::can_narrow`].
    pub fn narrow(&mut self, units: &[Unit]) -> Vec<String> {
        if !self.can_narrow() {
            return self.enabled.clone();
        }
        self.partner_suspects = self.partners.clone();
        self.note = None;
        self.next_partner_step(units)
    }

    fn involved_closure(&self, units: &[Unit]) -> HashSet<String> {
        self.involved
            .as_ref()
            .map(|key| closure(units, &[key.clone()].into_iter().collect()))
            .unwrap_or_default()
    }

    /// Halves the candidates for the other mod, or checks the last one with
    /// the involved mod on their own.
    fn next_partner_step(&mut self, units: &[Unit]) -> Vec<String> {
        match self.partner_suspects.len() {
            0 => return self.needs_more_than_two(units),
            1 => {
                let partner = self.partner_suspects[0].clone();
                let mut seed: HashSet<String> = [partner].into_iter().collect();
                seed.extend(self.involved.iter().cloned());
                let pair = closure(units, &seed);
                let seen = self
                    .last_present
                    .as_ref()
                    .is_some_and(|present| present.iter().all(|key| pair.contains(key)));
                if seen {
                    return self.found_pair(units);
                }
                self.enabled = sorted(&pair);
                self.phase = Phase::PartnerConfirm;
                return self.enabled.clone();
            }
            _ => {}
        }
        self.partner_suspects = by_closure_size(units, &self.partner_suspects);
        let half = self.partner_suspects.len().div_ceil(2);
        let mut seed: HashSet<String> = self.partner_suspects.iter().take(half).cloned().collect();
        seed.extend(self.involved.iter().cloned());
        let test = closure(units, &seed);
        if self.partner_suspects.iter().all(|key| test.contains(key)) {
            // The candidates cannot be split (each needs the others).
            return self.needs_more_than_two(units);
        }
        self.enabled = sorted(&test);
        self.phase = Phase::PartnerSearch;
        self.enabled.clone()
    }

    fn found_pair(&mut self, units: &[Unit]) -> Vec<String> {
        self.phase = Phase::FoundPair;
        self.partner = self.partner_suspects.first().cloned();
        // Everything back on except the involved mod (and what needs it), so
        // playing now checks the answer.
        let involved: HashSet<String> = self.involved.iter().cloned().collect();
        let without: HashSet<String> = units
            .iter()
            .filter(|unit| {
                closure(units, &[unit.key.clone()].into_iter().collect()).is_disjoint(&involved)
            })
            .map(|unit| unit.key.clone())
            .collect();
        self.enabled = sorted(&without);
        self.note = Some(
            "These two together are the most likely cause: the problem happened with only them on, and not with either of them without the other. Check it before deciding."
                .to_string(),
        );
        self.enabled.clone()
    }

    fn needs_more_than_two(&mut self, units: &[Unit]) -> Vec<String> {
        self.phase = Phase::Inconclusive;
        self.partners = self.partner_suspects.clone();
        self.partner_suspects.clear();
        self.note = Some(
            "No single other mod completes it: the problem seems to need three or more mods on together."
                .to_string(),
        );
        self.enabled = sorted(&closure(units, &self.known_good.iter().cloned().collect()));
        self.enabled.clone()
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
        self.suspects = by_closure_size(units, &self.suspects);

        let half = self.suspects.len().div_ceil(2);
        let mut seed: HashSet<String> = self.suspects.iter().take(half).cloned().collect();
        seed.extend(self.known_good.iter().cloned());
        let test = closure(units, &seed);
        if self.suspects.iter().all(|key| test.contains(key)) {
            // Only possible when requirements loop (see `collapse_cycles`):
            // the same test would be asked for again and again.
            self.phase = Phase::Inconclusive;
            self.note = Some(
                "The remaining mods need each other, so they cannot be tested apart.".to_string(),
            );
            self.enabled = sorted(&closure(units, &self.known_good.iter().cloned().collect()));
            return;
        }
        self.enabled = sorted(&test);
        self.phase = Phase::Testing;
    }

    /// Records whether the problem is present with the current test and moves to
    /// the next one. Returns the units that should be enabled afterwards.
    pub fn answer(&mut self, units: &[Unit], problem_present: bool) -> Vec<String> {
        match self.phase {
            Phase::Found | Phase::Inconclusive | Phase::FoundPair => return self.enabled.clone(),
            Phase::PartnerSearch => {
                self.steps += 1;
                let tested: HashSet<String> = self.enabled.iter().cloned().collect();
                if problem_present {
                    self.last_present = Some(self.enabled.clone());
                    self.partner_suspects.retain(|key| tested.contains(key));
                } else {
                    self.partner_suspects.retain(|key| !tested.contains(key));
                }
                return self.next_partner_step(units);
            }
            Phase::PartnerConfirm => {
                self.steps += 1;
                return if problem_present {
                    self.last_present = Some(self.enabled.clone());
                    self.found_pair(units)
                } else {
                    self.needs_more_than_two(units)
                };
            }
            Phase::Confirming => {
                self.steps += 1;
                return if problem_present {
                    self.last_present = Some(self.enabled.clone());
                    self.conclude(units)
                } else {
                    self.combination(units)
                };
            }
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
                // The session began because the problem happens with every
                // mod on.
                self.last_present = Some(self.suspects.clone());
                if self.suspects.len() <= 1 {
                    // The problem was there with this one mod on, and is gone
                    // with it off: nothing else could be involved.
                    return self.conclude(units);
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
            self.last_present = Some(self.enabled.clone());
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
            1 => return self.confirm(units),
            _ => self.next_test(units),
        }
        self.enabled.clone()
    }

    /// The last suspect: named at once when the problem has already been seen
    /// with nothing but it (and what it requires) on, otherwise tested alone.
    fn confirm(&mut self, units: &[Unit]) -> Vec<String> {
        let Some(candidate) = self.suspects.first().cloned() else {
            return self.enabled.clone();
        };
        let alone = closure(units, &[candidate].into_iter().collect());
        let seen_alone = self
            .last_present
            .as_ref()
            .is_some_and(|present| present.iter().all(|key| alone.contains(key)));
        if seen_alone {
            return self.conclude(units);
        }
        self.enabled = sorted(&alone);
        self.phase = Phase::Confirming;
        self.enabled.clone()
    }

    /// The suspect causes the problem on its own. Every other mod goes back on
    /// with it (and anything that needs it) off, so playing now is the check
    /// the result asks for.
    fn conclude(&mut self, units: &[Unit]) -> Vec<String> {
        self.phase = Phase::Found;
        self.culprit = self.suspects.first().cloned();
        let culprit: HashSet<String> = self.culprit.iter().cloned().collect();
        let without: HashSet<String> = units
            .iter()
            .filter(|unit| {
                let seed: HashSet<String> = [unit.key.clone()].into_iter().collect();
                closure(units, &seed).is_disjoint(&culprit)
            })
            .map(|unit| unit.key.clone())
            .collect();
        self.enabled = sorted(&without);
        self.note = Some(
            "This is the most likely cause: the problem happened with it on by itself, and not in the tests without it. Check it before deciding."
                .to_string(),
        );
        self.enabled.clone()
    }

    /// The last suspect does not cause the problem by itself, although the
    /// problem happened with it on together with mods that are fine alone. No
    /// single mod is to blame.
    fn combination(&mut self, units: &[Unit]) -> Vec<String> {
        self.phase = Phase::Inconclusive;
        let involved = self.suspects.first().cloned();
        self.involved = involved;
        let alone = self.involved_closure(units);
        let mut partners: Vec<String> = self
            .last_present
            .iter()
            .flatten()
            .filter(|key| !alone.contains(*key))
            .cloned()
            .collect();
        partners.sort();
        self.partners = partners;
        self.note = Some(
            "The problem needs two or more mods on together: it went away with the last suspect on by itself. No single mod is to blame."
                .to_string(),
        );
        self.enabled = sorted(&closure(units, &self.known_good.iter().cloned().collect()));
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
            // One step for the all-off check, about log2(20) tests, and at
            // most one more to see the last suspect cause it on its own.
            assert!(steps <= 8, "took {steps} steps for {culprit}");
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

    /// Runs a whole session where the problem happens only when every one of
    /// `together` is enabled.
    fn run_combination(units: &[Unit], together: &[&str]) -> Session {
        let mut session = Session::start();
        let mut enabled: Vec<String> = Vec::new();
        for _ in 0..64 {
            let present = together.iter().all(|k| enabled.iter().any(|e| e == k));
            enabled = session.answer(units, present);
            if matches!(session.phase, Phase::Found | Phase::Inconclusive) {
                break;
            }
        }
        session
    }

    #[test]
    fn a_combination_of_two_mods_is_reported_as_inconclusive_not_misattributed() {
        // The problem needs both x and y; no single one explains it.
        let units: Vec<Unit> = ["x", "y", "p", "q"].iter().map(|k| unit(k, &[])).collect();
        let session = run_combination(&units, &["x", "y"]);
        assert_eq!(session.phase, Phase::Inconclusive);
        assert!(session.culprit.is_none());
    }

    #[test]
    fn no_pair_is_ever_blamed_on_one_mod_and_both_are_named() {
        let keys: Vec<String> = (0..13).map(|i| format!("m{i:02}")).collect();
        let units: Vec<Unit> = keys.iter().map(|k| unit(k, &[])).collect();
        for (i, first) in keys.iter().enumerate() {
            for second in &keys[i + 1..] {
                let session = run_combination(&units, &[first, second]);
                assert_eq!(
                    session.phase,
                    Phase::Inconclusive,
                    "pair {first}+{second} gave {:?}",
                    session.culprit
                );
                assert!(session.culprit.is_none());
                let involved = session.involved.clone().expect("a mod is named");
                let mut named = session.partners.clone();
                named.push(involved);
                assert!(
                    named.contains(first) && named.contains(second),
                    "pair {first}+{second} named {named:?}"
                );
            }
        }
    }

    #[test]
    fn the_pair_from_the_exploratory_test_ends_inconclusive() {
        // Thirteen mods, three with a requirement, as in the report: the
        // problem appears only with Weather.Plus and Fish.Easy both loaded.
        let units = vec![
            unit("Automate.Chests", &[]),
            unit("Better.Tools", &["Core.Lib"]),
            unit("Core.Lib", &[]),
            unit("Content.Patcher", &[]),
            unit("Fancy.Farm", &["Content.Patcher"]),
            unit("More.Crops", &["Content.Patcher"]),
            unit("Lights.Lib", &[]),
            unit("Street.Lamps", &["Lights.Lib"]),
            unit("Weather.Plus", &[]),
            unit("Fish.Easy", &[]),
            unit("Ui.Clock", &[]),
            unit("Romance.Plus", &[]),
            unit("Big.Map", &[]),
        ];
        let session = run_combination(&units, &["Weather.Plus", "Fish.Easy"]);
        assert_eq!(session.phase, Phase::Inconclusive);
        assert!(session.culprit.is_none());
        let mut named = session.partners.clone();
        named.extend(session.involved.clone());
        assert!(named.contains(&"Weather.Plus".to_string()));
        assert!(named.contains(&"Fish.Easy".to_string()));
    }

    #[test]
    fn a_found_culprit_leaves_every_other_mod_on_so_the_advice_can_be_checked() {
        let units = vec![
            unit("a", &["lib"]),
            unit("b", &[]),
            unit("c", &[]),
            unit("d", &["c"]),
            unit("lib", &[]),
        ];
        for culprit in ["a", "b", "c", "lib"] {
            let mut session = Session::start();
            let mut enabled: Vec<String> = Vec::new();
            for _ in 0..16 {
                let present = enabled.iter().any(|k| k == culprit);
                enabled = session.answer(&units, present);
                if matches!(session.phase, Phase::Found | Phase::Inconclusive) {
                    break;
                }
            }
            assert_eq!(session.phase, Phase::Found, "culprit {culprit}");
            assert_eq!(session.culprit.as_deref(), Some(culprit));
            // Only the culprit, and whatever cannot run without it, is off.
            let set: HashSet<String> = enabled.iter().cloned().collect();
            assert_eq!(closure(&units, &set), set, "open set {enabled:?}");
            assert!(!set.contains(culprit));
            let needs_it: Vec<&str> = match culprit {
                "c" => vec!["c", "d"],
                "lib" => vec!["a", "lib"],
                other => vec![other],
            };
            for unit in &units {
                assert_eq!(
                    set.contains(&unit.key),
                    !needs_it.contains(&unit.key.as_str()),
                    "culprit {culprit}, unit {}",
                    unit.key
                );
            }
        }
    }

    #[test]
    fn a_single_mod_profile_is_concluded_immediately_after_the_baseline() {
        let units = vec![unit("only", &[])];
        let mut session = Session::start();
        session.answer(&units, false);
        assert_eq!(session.phase, Phase::Found);
        assert_eq!(session.culprit.as_deref(), Some("only"));
    }

    /// Runs a session to its end, then (when it ends inconclusive with a mod
    /// involved) looks for the other mod. The problem happens when every one
    /// of `together` is on. Returns the session and the steps taken.
    fn run_and_narrow(units: &[Unit], together: &[&str]) -> Session {
        let mut session = run_combination(units, together);
        let mut enabled = session.narrow(units);
        for _ in 0..64 {
            if !matches!(session.phase, Phase::PartnerSearch | Phase::PartnerConfirm) {
                break;
            }
            let present = together.iter().all(|k| enabled.iter().any(|e| e == k));
            enabled = session.answer(units, present);
        }
        session
    }

    #[test]
    fn requirements_written_in_another_case_still_travel_together() {
        // A content pack naming its host in lower case, and a mod naming a
        // dependency in upper case: SMAPI matches both.
        let mut pack = unit("pack", &[]);
        pack.requires = vec!["pathoschild.contentpatcher".to_string()];
        let mut cp = unit("cp", &[]);
        cp.unique_ids = vec!["Pathoschild.ContentPatcher".to_string()];
        let mut needy = unit("needy", &[]);
        needy.requires = vec!["ID.LIB".to_string()];
        let mut units = vec![pack, cp, needy, unit("lib", &[])];
        units.extend((0..6).map(|i| unit(&format!("m{i:03}"), &[])));
        assert_eq!(
            sorted(&closure(
                &units,
                &["pack".to_string()].into_iter().collect()
            )),
            vec!["cp", "pack"]
        );
        assert_eq!(
            sorted(&closure(
                &units,
                &["needy".to_string()].into_iter().collect()
            )),
            vec!["lib", "needy"]
        );
        for culprit in ["pack", "needy"] {
            let (session, _) = find(&units, culprit);
            assert_eq!(session.phase, Phase::Found, "culprit {culprit}");
            assert_eq!(session.culprit.as_deref(), Some(culprit));
        }
    }

    #[test]
    fn a_requirement_loop_never_makes_a_session_repeat_itself() {
        // a and b require each other. Before, every half's closure was the
        // whole set and the same test was asked for forever.
        let units = vec![unit("a", &["b"]), unit("b", &["a"]), unit("c", &[])];
        for culprit in ["a", "c"] {
            let mut session = Session::start();
            let mut enabled: Vec<String> = Vec::new();
            for _ in 0..64 {
                let present = enabled.iter().any(|k| k == culprit);
                enabled = session.answer(&units, present);
                if matches!(session.phase, Phase::Found | Phase::Inconclusive) {
                    break;
                }
            }
            assert!(
                matches!(session.phase, Phase::Found | Phase::Inconclusive),
                "culprit {culprit}: still {:?} after {} steps",
                session.phase,
                session.steps
            );
            assert!(session.steps <= 6, "{} steps", session.steps);
        }
        // Collapsed, the loop is one unit and is found like any other.
        let collapsed = collapse_cycles(&units);
        let keys: Vec<&str> = collapsed.iter().map(|u| u.key.as_str()).collect();
        assert_eq!(keys, vec!["a+b", "c"]);
        assert!(collapsed[0].requires.is_empty());
        for culprit in ["a+b", "c"] {
            let (session, _) = find(&collapsed, culprit);
            assert_eq!(session.phase, Phase::Found, "culprit {culprit}");
            assert_eq!(session.culprit.as_deref(), Some(culprit));
        }
        assert_eq!(group_members("a+b").collect::<Vec<_>>(), vec!["a", "b"]);
    }

    #[test]
    fn the_other_mod_of_a_pair_is_found_in_a_few_more_steps() {
        // Every pair for 13 and 30 mods, and pairs starting at every 25th
        // mod for 100 (all 4950 take a while). The worst cases (main search
        // plus finding the other mod) are in docs/operations-and-recovery.md:
        // 13 mods 11, 30 mods 13, 100 mods 17 steps, about 2 log2(n).
        for (n, limit, stride) in [(13usize, 11u32, 1usize), (30, 13, 1), (100, 17, 25)] {
            let keys: Vec<String> = (0..n).map(|i| format!("m{i:03}")).collect();
            let units: Vec<Unit> = keys.iter().map(|k| unit(k, &[])).collect();
            let mut worst = 0;
            for (i, first) in keys.iter().enumerate().step_by(stride) {
                for second in &keys[i + 1..] {
                    let session = run_and_narrow(&units, &[first, second]);
                    assert_eq!(session.phase, Phase::FoundPair, "pair {first}+{second}");
                    let mut named = vec![
                        session.involved.clone().unwrap(),
                        session.partner.clone().unwrap(),
                    ];
                    named.sort();
                    assert_eq!(named, vec![first.clone(), second.clone()]);
                    assert!(session.culprit.is_none());
                    worst = worst.max(session.steps);
                }
            }
            assert!(
                worst <= limit,
                "{n} mods: worst {worst} steps, limit {limit}"
            );
        }
    }

    #[test]
    fn a_found_pair_leaves_everything_on_but_one_of_them() {
        let units: Vec<Unit> = ["x", "y", "p", "q", "r"]
            .iter()
            .map(|k| unit(k, &[]))
            .collect();
        let session = run_and_narrow(&units, &["x", "y"]);
        assert_eq!(session.phase, Phase::FoundPair);
        let off: Vec<&String> = units
            .iter()
            .map(|u| &u.key)
            .filter(|k| !session.enabled.contains(k))
            .collect();
        assert_eq!(off, vec![session.involved.as_ref().unwrap()]);
    }

    #[test]
    fn three_mods_needed_together_are_not_reported_as_a_pair() {
        let keys: Vec<String> = (0..13).map(|i| format!("m{i:02}")).collect();
        let units: Vec<Unit> = keys.iter().map(|k| unit(k, &[])).collect();
        let session = run_and_narrow(&units, &["m01", "m05", "m09"]);
        assert_eq!(session.phase, Phase::Inconclusive);
        assert!(session.partner.is_none());
        assert!(session.culprit.is_none());
        assert!(session
            .note
            .as_deref()
            .unwrap_or("")
            .contains("three or more"));
    }

    #[test]
    fn narrowing_is_offered_only_for_an_involved_mod() {
        let units = vec![unit("a", &[]), unit("b", &[])];
        let mut session = Session::start();
        session.answer(&units, true);
        assert!(!session.can_narrow(), "probably not a mod");
        let before = session.clone();
        session.narrow(&units);
        assert_eq!(session, before);
    }
}
