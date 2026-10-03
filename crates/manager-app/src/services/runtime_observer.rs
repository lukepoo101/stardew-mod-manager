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

fn override_key(game_id: &GameInstallationId) -> String {
    format!("detection_override:game_version:{game_id}")
}

/// A game version the user set because detection got it wrong or could not
/// read it. Only this detection result can be overridden; paths, ownership
/// and safety checks never can.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GameVersionOverride {
    pub value: String,
    pub reason: String,
    pub set_at: String,
    /// What detection said when the override was set, so a later change in
    /// what is detected marks the override as needing a look.
    pub observed_then: Option<String>,
}

/// An override with what detection says now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideStatus {
    pub game_version: GameVersionOverride,
    pub observed_now: Option<String>,
}

impl OverrideStatus {
    /// Detection changed since the override was set.
    pub fn is_stale(&self) -> bool {
        self.observed_now != self.game_version.observed_then
    }
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

    /// The game version detection reads, ignoring any override.
    pub fn detected_game_version(&self, game_id: &GameInstallationId) -> AppResult<Option<String>> {
        let Some(game) = self.game_repo.get_game(game_id)? else {
            return Ok(None);
        };
        Ok(self
            .inspector
            .inspect(&game.canonical_root, game.storefront, game.operating_system)
            .ok()
            .and_then(|inspection| inspection.observed_game_version))
    }

    pub fn game_version_override(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Option<OverrideStatus>> {
        let Some(stored) = self
            .preferences
            .get_preference(&override_key(game_id))?
            .filter(|json| !json.is_empty())
            .and_then(|json| serde_json::from_str::<GameVersionOverride>(&json).ok())
        else {
            return Ok(None);
        };
        Ok(Some(OverrideStatus {
            game_version: stored,
            observed_now: self.detected_game_version(game_id)?,
        }))
    }

    /// Sets the game version to use instead of the detected one, or clears it.
    pub fn set_game_version_override(
        &self,
        game_id: &GameInstallationId,
        value: Option<(&str, &str)>,
    ) -> AppResult<()> {
        let Some((version, reason)) = value else {
            return self.preferences.set_preference(&override_key(game_id), "");
        };
        let version = version.trim();
        let plausible = !version.is_empty()
            && version.len() <= 32
            && version.split('.').all(|part| {
                !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            });
        if !plausible {
            return Err(crate::error::AppError::validation(
                "GAME_VERSION_INVALID",
                "Enter a version such as 1.6.15",
            ));
        }
        let stored = GameVersionOverride {
            value: version.to_string(),
            reason: reason.trim().chars().take(200).collect(),
            set_at: chrono::Utc::now().to_rfc3339(),
            observed_then: self.detected_game_version(game_id)?,
        };
        let json = serde_json::to_string(&stored).map_err(|e| {
            crate::error::AppError::internal("Could not save the override", e.to_string())
        })?;
        self.preferences
            .set_preference(&override_key(game_id), &json)
    }

    /// The versions in place right now. Anything that cannot be read is `None`.
    /// A game version the user set overrides the detected one.
    pub fn observe(&self, game_id: &GameInstallationId) -> AppResult<RuntimeVersions> {
        if self.game_repo.get_game(game_id)?.is_none() {
            return Ok(RuntimeVersions::default());
        }
        let game_version = match self.game_version_override(game_id)? {
            Some(status) => Some(status.game_version.value),
            None => self.detected_game_version(game_id)?,
        };
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
        if let Ok(mut value) = serde_json::to_value(versions) {
            // When, alongside what, so a change can say how old the
            // last-worked evidence is.
            value["recorded_at"] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());
            self.preferences
                .set_preference(&key(profile_id), &value.to_string())?;
        }
        Ok(())
    }

    /// When the versions returned by `recall` were recorded, if known.
    pub fn recalled_at(&self, profile_id: &ProfileId) -> AppResult<Option<String>> {
        Ok(self
            .preferences
            .get_preference(&key(profile_id))?
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .and_then(|value| value.get("recorded_at")?.as_str().map(str::to_string)))
    }

    /// The versions a profile last worked with, if any were recorded and readable.
    pub fn recall(&self, profile_id: &ProfileId) -> AppResult<Option<RuntimeVersions>> {
        Ok(self
            .preferences
            .get_preference(&key(profile_id))?
            .and_then(|json| serde_json::from_str(&json).ok()))
    }
}
