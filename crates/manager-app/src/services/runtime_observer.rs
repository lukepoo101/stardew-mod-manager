use crate::error::AppResult;
use crate::ports::discovery::GameInstallationInspectorPort;
use crate::ports::repositories::{
    GameInstallationRepository, PreferencesRepository, SmapiRepository,
};
use manager_core::ids::{GameInstallationId, ProfileId};
use manager_core::launch::RuntimeVersions;
use std::sync::Arc;

/// Observes the game and SMAPI versions and remembers, per profile, the ones it
/// last ran successfully with.
///
/// The record lives in the preferences store: it is an observation about a
/// profile, not part of any launch session, and losing it only loses a warning.
pub struct RuntimeObserver {
    game_repo: Arc<dyn GameInstallationRepository>,
    smapi_repo: Arc<dyn SmapiRepository>,
    preferences: Arc<dyn PreferencesRepository>,
    inspector: Arc<dyn GameInstallationInspectorPort>,
}

fn key(profile_id: &ProfileId) -> String {
    format!("last_working_runtime:{}", profile_id)
}

impl RuntimeObserver {
    pub fn new(
        game_repo: Arc<dyn GameInstallationRepository>,
        smapi_repo: Arc<dyn SmapiRepository>,
        preferences: Arc<dyn PreferencesRepository>,
        inspector: Arc<dyn GameInstallationInspectorPort>,
    ) -> Self {
        Self {
            game_repo,
            smapi_repo,
            preferences,
            inspector,
        }
    }

    /// The versions in place right now. Anything that cannot be read is `None`.
    pub fn observe(&self, game_id: &GameInstallationId) -> AppResult<RuntimeVersions> {
        let Some(game) = self.game_repo.get_game(game_id)? else {
            return Ok(RuntimeVersions::default());
        };
        let game_version = self
            .inspector
            .inspect(&game.canonical_root, game.storefront, game.operating_system)
            .ok()
            .and_then(|inspection| inspection.observed_game_version);
        let smapi_version = self
            .smapi_repo
            .get_smapi_installation(game_id)?
            .map(|installation| installation.release_version);
        Ok(RuntimeVersions {
            game_version,
            smapi_version,
        })
    }

    /// Records the versions a profile just worked with.
    pub fn remember(&self, profile_id: &ProfileId, versions: &RuntimeVersions) -> AppResult<()> {
        if let Ok(json) = serde_json::to_string(versions) {
            self.preferences.set_preference(&key(profile_id), &json)?;
        }
        Ok(())
    }

    /// The versions a profile last worked with, if any were recorded and readable.
    pub fn recall(&self, profile_id: &ProfileId) -> AppResult<Option<RuntimeVersions>> {
        Ok(self
            .preferences
            .get_preference(&key(profile_id))?
            .and_then(|json| serde_json::from_str(&json).ok()))
    }
}
