//! The last state a profile was seen working in.
//!
//! "Working" has one meaning here: a modded session whose mods SMAPI was
//! confirmed to have loaded. Only then is the profile's mod list, with the game
//! and SMAPI versions, recorded; starting the game is not enough. The record is
//! an observation kept in the preferences store and never changes anything.

use crate::api::dto::{BaselineFindingDto, KnownGoodDto};
use crate::error::AppResult;
use crate::ports::repositories::{
    DeploymentRepository, PackageCatalogRepository, PreferencesRepository,
};
use crate::services::freeze::snapshot_mods;
use crate::services::HealthService;
use chrono::Utc;
use manager_core::ids::ProfileId;
use manager_core::launch::RuntimeVersions;
use std::sync::Arc;

fn key(profile_id: &ProfileId) -> String {
    format!("known_good:{profile_id}")
}

/// The profile's last working state, if one was recorded.
pub fn stored_known_good(
    preferences: &dyn PreferencesRepository,
    profile_id: &ProfileId,
) -> AppResult<Option<KnownGoodDto>> {
    Ok(preferences
        .get_preference(&key(profile_id))?
        .and_then(|json| serde_json::from_str(&json).ok()))
}

pub struct KnownGood {
    preferences: Arc<dyn PreferencesRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    health: Option<Arc<HealthService>>,
}

impl KnownGood {
    pub fn new(
        preferences: Arc<dyn PreferencesRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
    ) -> Self {
        Self {
            preferences,
            deployment_repo,
            package_repo,
            health: None,
        }
    }

    /// Also keeps the health findings at the moment the profile worked, so
    /// later findings can be compared with them.
    pub fn with_health(mut self, health: Arc<HealthService>) -> Self {
        self.health = Some(health);
        self
    }

    /// Records the profile as it is now, with the runtime it just worked with.
    pub fn record(&self, profile_id: &ProfileId, runtime: &RuntimeVersions) -> AppResult<()> {
        let snapshot = KnownGoodDto {
            profile_id: profile_id.to_string(),
            recorded_at: Utc::now().to_rfc3339(),
            game_version: runtime.game_version.clone(),
            smapi_version: runtime.smapi_version.clone(),
            mods: snapshot_mods(&*self.deployment_repo, &*self.package_repo, profile_id)?,
            // A health check that fails leaves the baseline unknown, not empty.
            findings: self.health.as_ref().and_then(|health| {
                health
                    .get_health_summary(Some(profile_id))
                    .ok()
                    .map(|summary| {
                        summary
                            .findings
                            .into_iter()
                            .map(|f| BaselineFindingDto {
                                fingerprint: f.fingerprint,
                                code: f.code,
                                severity: f.severity,
                                title: f.title,
                            })
                            .collect()
                    })
            }),
        };
        if let Ok(json) = serde_json::to_string(&snapshot) {
            self.preferences.set_preference(&key(profile_id), &json)?;
        }
        Ok(())
    }

    pub fn get(&self, profile_id: &ProfileId) -> AppResult<Option<KnownGoodDto>> {
        stored_known_good(&*self.preferences, profile_id)
    }
}
