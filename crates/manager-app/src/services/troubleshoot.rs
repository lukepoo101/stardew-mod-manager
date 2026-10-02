//! Applies a bisection session to a profile, and remembers how to undo it.
//!
//! The original enabled state is written down before the first mod is turned
//! off, and restoring re-applies exactly that, so a troubleshooting session can
//! always be abandoned without losing the user's setup.

use crate::api::dto::{TroubleshootDto, TroubleshootStepDto};
use crate::error::{AppError, AppResult};
use crate::ports::repositories::{
    DeploymentRepository, LaunchSessionRepository, PackageCatalogRepository, PreferencesRepository,
};
use crate::services::toggle::ToggleService;
use manager_core::ids::{ProfileComponentId, ProfileId};
use manager_core::troubleshoot::{Phase, Session, Unit};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
struct Stored {
    /// Every profile component and whether it was enabled before the session.
    original: Vec<(String, bool)>,
    session: Session,
    /// When the step in progress began, to find the game session that
    /// tested it.
    #[serde(default)]
    step_started_at: Option<String>,
    #[serde(default)]
    history: Vec<TroubleshootStepDto>,
}

pub struct TroubleshootService {
    toggle: Arc<ToggleService>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    preferences: Arc<dyn PreferencesRepository>,
    sessions: Option<Arc<dyn LaunchSessionRepository>>,
}

fn key(profile_id: &ProfileId) -> String {
    format!("troubleshoot:{}", profile_id)
}

#[derive(Default)]
struct UnitFacts {
    unique_ids: Vec<String>,
    names: Vec<String>,
    requires: Vec<String>,
}

struct Snapshot {
    units: Vec<Unit>,
    /// Display name and one profile component id per unit key.
    names: HashMap<String, String>,
    representative: HashMap<String, ProfileComponentId>,
    components: Vec<(ProfileComponentId, bool)>,
}

/// Why mods in the enabled set are on together: several mods from one
/// download, and mods kept on because an enabled mod needs them.
fn together(snapshot: &Snapshot, enabled: &[String]) -> Vec<String> {
    let enabled: HashSet<&String> = enabled.iter().collect();
    let mut lines = Vec::new();
    let mut provider: HashMap<String, &Unit> = HashMap::new();
    for unit in &snapshot.units {
        for id in &unit.unique_ids {
            provider.insert(id.to_lowercase(), unit);
        }
    }
    for unit in snapshot.units.iter().filter(|u| enabled.contains(&u.key)) {
        let name = snapshot.names.get(&unit.key).cloned().unwrap_or_default();
        if unit.unique_ids.len() > 1 {
            lines.push(format!(
                "{name} come from one download, so they are tested together."
            ));
        }
        let mut needs: Vec<String> = unit
            .requires
            .iter()
            .filter_map(|id| provider.get(&id.to_lowercase()))
            .filter(|needed| needed.key != unit.key && enabled.contains(&needed.key))
            .map(|needed| snapshot.names.get(&needed.key).cloned().unwrap_or_default())
            .collect();
        needs.sort();
        needs.dedup();
        if !needs.is_empty() {
            lines.push(format!(
                "{} stay on while {name} is on, because it needs them.",
                needs.join(", ")
            ));
        }
    }
    lines
}

impl TroubleshootService {
    pub fn new(
        toggle: Arc<ToggleService>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        preferences: Arc<dyn PreferencesRepository>,
    ) -> Self {
        Self {
            toggle,
            deployment_repo,
            package_repo,
            preferences,
            sessions: None,
        }
    }

    /// Links each answered step to the game session that tested it.
    pub fn with_sessions(mut self, sessions: Arc<dyn LaunchSessionRepository>) -> Self {
        self.sessions = Some(sessions);
        self
    }

    /// The latest session of the profile started since `since`.
    fn session_since(
        &self,
        profile_id: &ProfileId,
        since: Option<&str>,
    ) -> AppResult<Option<(String, String)>> {
        let Some(sessions) = &self.sessions else {
            return Ok(None);
        };
        let Some(since) = since.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()) else {
            return Ok(None);
        };
        Ok(sessions
            .get_latest_launch_session(Some(profile_id))?
            .filter(|session| session.launched_at >= since)
            .map(|session| {
                (
                    session.id.to_string(),
                    serde_json::to_value(session.state)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default(),
                )
            }))
    }

    fn snapshot(&self, profile_id: &ProfileId) -> AppResult<Snapshot> {
        let mut grouped: BTreeMap<String, UnitFacts> = BTreeMap::new();
        let mut representative = HashMap::new();
        let mut components = Vec::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            components.push((pc.id, pc.enabled));
            let unit_key = pc.deployment_id.to_string();
            representative.entry(unit_key.clone()).or_insert(pc.id);
            let Some(component) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            let entry = grouped.entry(unit_key).or_default();
            entry.unique_ids.push(component.unique_id.to_string());
            entry.names.push(component.name.clone());
            for dependency in component
                .manifest
                .dependencies
                .iter()
                .filter(|d| d.is_required)
            {
                entry.requires.push(dependency.unique_id.to_string());
            }
            if let Some(host) = &component.manifest.content_pack_for {
                entry.requires.push(host.unique_id.to_string());
            }
        }
        let mut units = Vec::new();
        let mut names = HashMap::new();
        for (unit_key, facts) in grouped {
            names.insert(unit_key.clone(), facts.names.join(", "));
            units.push(Unit {
                key: unit_key,
                unique_ids: facts.unique_ids,
                requires: facts.requires,
            });
        }
        Ok(Snapshot {
            units,
            names,
            representative,
            components,
        })
    }

    fn load(&self, profile_id: &ProfileId) -> AppResult<Option<Stored>> {
        Ok(self
            .preferences
            .get_preference(&key(profile_id))?
            .filter(|json| !json.is_empty())
            .and_then(|json| serde_json::from_str(&json).ok()))
    }

    fn store(&self, profile_id: &ProfileId, stored: Option<&Stored>) -> AppResult<()> {
        let json = match stored {
            Some(stored) => serde_json::to_string(stored)
                .map_err(|e| AppError::internal("Could not save the session", e.to_string()))?,
            None => String::new(),
        };
        self.preferences.set_preference(&key(profile_id), &json)
    }

    /// Enables exactly the given units and disables the rest.
    fn apply(&self, snapshot: &Snapshot, enabled: &HashSet<String>) -> AppResult<()> {
        for unit in &snapshot.units {
            if let Some(id) = snapshot.representative.get(&unit.key) {
                self.toggle.set_enabled(id, enabled.contains(&unit.key))?;
            }
        }
        Ok(())
    }

    fn describe(&self, profile_id: &ProfileId) -> AppResult<TroubleshootDto> {
        let Some(stored) = self.load(profile_id)? else {
            return Ok(TroubleshootDto::inactive());
        };
        let snapshot = self.snapshot(profile_id)?;
        let name = |unit_key: &String| {
            snapshot
                .names
                .get(unit_key)
                .cloned()
                .unwrap_or_else(|| unit_key.clone())
        };
        let session = &stored.session;
        Ok(TroubleshootDto {
            active: true,
            phase: match session.phase {
                Phase::AllOff => "all_off",
                Phase::Testing => "testing",
                Phase::Found => "found",
                Phase::Inconclusive => "inconclusive",
            }
            .to_string(),
            step: session.steps,
            suspects: session.suspects.iter().map(name).collect(),
            enabled_mods: session
                .enabled
                .iter()
                .map(|k| snapshot.names.get(k).cloned().unwrap_or_else(|| k.clone()))
                .collect(),
            culprit: session.culprit.as_ref().map(name),
            note: session.note.clone(),
            together: together(&snapshot, &session.enabled),
            history: stored.history.clone(),
        })
    }

    pub fn status(&self, profile_id: &ProfileId) -> AppResult<TroubleshootDto> {
        self.describe(profile_id)
    }

    pub fn start(&self, profile_id: &ProfileId) -> AppResult<TroubleshootDto> {
        if self.load(profile_id)?.is_some() {
            return Err(AppError::validation(
                "TROUBLESHOOT_ACTIVE",
                "A troubleshooting session is already in progress for this profile",
            ));
        }
        let snapshot = self.snapshot(profile_id)?;
        if snapshot.units.is_empty() {
            return Err(AppError::validation(
                "NO_MODS",
                "There are no mods in this profile to troubleshoot",
            ));
        }
        let stored = Stored {
            original: snapshot
                .components
                .iter()
                .map(|(id, enabled)| (id.to_string(), *enabled))
                .collect(),
            session: Session::start(),
            step_started_at: Some(chrono::Utc::now().to_rfc3339()),
            history: Vec::new(),
        };
        // Written first: if turning mods off is interrupted, restoring still knows
        // what the profile looked like.
        self.store(profile_id, Some(&stored))?;
        self.apply(&snapshot, &HashSet::new())?;
        self.describe(profile_id)
    }

    pub fn answer(
        &self,
        profile_id: &ProfileId,
        problem_present: bool,
    ) -> AppResult<TroubleshootDto> {
        let mut stored = self.load(profile_id)?.ok_or_else(|| {
            AppError::validation(
                "TROUBLESHOOT_INACTIVE",
                "No troubleshooting session is active",
            )
        })?;
        let snapshot = self.snapshot(profile_id)?;
        let tested_by = self.session_since(profile_id, stored.step_started_at.as_deref())?;
        stored.history.push(TroubleshootStepDto {
            step: stored.session.steps,
            mods_on: stored.session.enabled.len() as u32,
            problem_present,
            answered_at: chrono::Utc::now().to_rfc3339(),
            session_id: tested_by.as_ref().map(|(id, _)| id.clone()),
            session_state: tested_by.map(|(_, state)| state),
        });
        stored.step_started_at = Some(chrono::Utc::now().to_rfc3339());
        // Only mods that were on can be behind the problem; ones the user had
        // already turned off stay off for the whole session.
        let original: HashMap<String, bool> = stored.original.iter().cloned().collect();
        let suspects_pool: Vec<Unit> = snapshot
            .units
            .iter()
            .filter(|unit| {
                snapshot
                    .representative
                    .get(&unit.key)
                    .map(|id| original.get(&id.to_string()).copied().unwrap_or(true))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let enabled: HashSet<String> = stored
            .session
            .answer(&suspects_pool, problem_present)
            .into_iter()
            .collect();
        self.apply(&snapshot, &enabled)?;
        self.store(profile_id, Some(&stored))?;
        self.describe(profile_id)
    }

    /// Puts every mod back the way it was before the session began.
    pub fn restore(&self, profile_id: &ProfileId) -> AppResult<TroubleshootDto> {
        let Some(stored) = self.load(profile_id)? else {
            return Ok(TroubleshootDto::inactive());
        };
        let snapshot = self.snapshot(profile_id)?;
        let original: HashMap<String, bool> = stored.original.iter().cloned().collect();
        // Restore per unit from the recorded per-component state; components in
        // one folder share a state, so the representative decides.
        for unit in &snapshot.units {
            let Some(id) = snapshot.representative.get(&unit.key) else {
                continue;
            };
            let was_enabled = original.get(&id.to_string()).copied().unwrap_or(true);
            self.toggle.set_enabled(id, was_enabled)?;
        }
        self.store(profile_id, None)?;
        Ok(TroubleshootDto::inactive())
    }
}
