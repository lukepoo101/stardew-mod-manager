use crate::ids::{GameInstallationId, ProfileId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileState {
    Active,
    Archived,
    Corrupted,
}

/// A first-class modding profile belonging to a game installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: ProfileId,
    pub game_installation_id: GameInstallationId,
    pub name: String,
    pub description: Option<String>,
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub state: ProfileState,
}

impl Profile {
    pub fn new(game_installation_id: GameInstallationId, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: ProfileId::new(),
            game_installation_id,
            name: name.into(),
            description: None,
            revision: 1,
            created_at: now,
            updated_at: now,
            state: ProfileState::Active,
        }
    }

    pub fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
        self.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingDisposition {
    NotStarted,
    Skipped,
    Completed,
}

/// Global application context tracking active installation and onboarding progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppContext {
    pub active_game_installation_id: Option<GameInstallationId>,
    pub onboarding_disposition: OnboardingDisposition,
}

impl Default for AppContext {
    fn default() -> Self {
        Self {
            active_game_installation_id: None,
            onboarding_disposition: OnboardingDisposition::NotStarted,
        }
    }
}

/// Profile context per game installation, tracking active and default profiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameProfileContext {
    pub game_installation_id: GameInstallationId,
    pub active_profile_id: Option<ProfileId>,
    pub default_profile_id: Option<ProfileId>,
    pub last_active_profile_id: Option<ProfileId>,
}
