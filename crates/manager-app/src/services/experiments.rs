//! Profiles marked as experiments: copies made to try changes safely.
//!
//! The mark only records where an experiment came from; the experiment is an
//! ordinary, independent profile (see `BundleService::clone_profile`), and
//! nothing ever flows back to the source. Keeping an experiment removes the
//! mark; discarding it is an ordinary archive and delete.

use crate::api::dto::ExperimentDto;
use crate::error::{AppError, AppResult};
use crate::ports::repositories::{PreferencesRepository, ProfileRepository};
use chrono::Utc;
use manager_core::ids::ProfileId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

const KEY: &str = "profile_experiments";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    source_profile_id: String,
    source_name: String,
    source_revision: u64,
    created_at: String,
}

pub struct ProfileExperiments {
    preferences: Arc<dyn PreferencesRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
}

impl ProfileExperiments {
    pub fn new(
        preferences: Arc<dyn PreferencesRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
    ) -> Self {
        Self {
            preferences,
            profile_repo,
        }
    }

    fn load(&self) -> AppResult<BTreeMap<String, Stored>> {
        Ok(self
            .preferences
            .get_preference(KEY)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    fn save(&self, map: &BTreeMap<String, Stored>) -> AppResult<()> {
        let json = serde_json::to_string(map)
            .map_err(|e| AppError::internal("Could not save the experiment", e.to_string()))?;
        self.preferences.set_preference(KEY, &json)
    }

    /// Experiments whose profile still exists.
    pub fn list(&self) -> AppResult<Vec<ExperimentDto>> {
        let mut out = Vec::new();
        for (id, stored) in self.load()? {
            let Ok(pid) = id.parse::<ProfileId>() else {
                continue;
            };
            if self.profile_repo.get_profile(&pid)?.is_none() {
                continue;
            }
            out.push(ExperimentDto {
                profile_id: id,
                source_profile_id: stored.source_profile_id,
                source_name: stored.source_name,
                source_revision: stored.source_revision,
                created_at: stored.created_at,
            });
        }
        Ok(out)
    }

    pub fn mark(&self, experiment: &ProfileId, source: &ProfileId) -> AppResult<ExperimentDto> {
        if experiment == source {
            return Err(AppError::validation(
                "EXPERIMENT_IS_SOURCE",
                "A profile cannot be an experiment on itself",
            ));
        }
        if self.profile_repo.get_profile(experiment)?.is_none() {
            return Err(AppError::validation(
                "PROFILE_NOT_FOUND",
                "The experiment profile does not exist",
            ));
        }
        let source_profile = self.profile_repo.get_profile(source)?.ok_or_else(|| {
            AppError::validation("PROFILE_NOT_FOUND", "The source profile does not exist")
        })?;
        let stored = Stored {
            source_profile_id: source.to_string(),
            source_name: source_profile.name,
            source_revision: source_profile.revision,
            created_at: Utc::now().to_rfc3339(),
        };
        let mut map = self.load()?;
        map.insert(experiment.to_string(), stored.clone());
        self.save(&map)?;
        Ok(ExperimentDto {
            profile_id: experiment.to_string(),
            source_profile_id: stored.source_profile_id,
            source_name: stored.source_name,
            source_revision: stored.source_revision,
            created_at: stored.created_at,
        })
    }

    /// Makes an experiment an ordinary profile, or forgets a discarded one.
    pub fn unmark(&self, experiment: &ProfileId) -> AppResult<()> {
        let mut map = self.load()?;
        if map.remove(&experiment.to_string()).is_some() {
            self.save(&map)?;
        }
        Ok(())
    }
}
