//! Freezing a profile at its current mods and versions.
//!
//! While frozen, nothing can be installed into or removed from the profile,
//! including a preview prepared before it was frozen. Enabling and disabling
//! stay allowed (troubleshooting depends on it); the frozen snapshot records
//! the enabled state so any drift is visible. Unfreezing changes nothing else.

use crate::api::dto::{FrozenModDto, ProfileFreezeDto, SettingFileHashDto};
use crate::error::{AppError, AppResult};
use crate::ports::repositories::{
    DeploymentRepository, PackageCatalogRepository, PreferencesRepository, ProfileRepository,
};
use chrono::Utc;
use manager_core::ids::ProfileId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

const KEY: &str = "frozen_profiles";
pub const MAX_REASON_CHARS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    frozen_at: String,
    reason: String,
    mods: Vec<FrozenModDto>,
    #[serde(default)]
    settings: Vec<SettingFileHashDto>,
    #[serde(default)]
    game_version: Option<String>,
    #[serde(default)]
    smapi_version: Option<String>,
}

/// What else a freeze records besides the mods: settings checksums and the
/// runtime last observed, as context.
#[derive(Debug, Clone, Default)]
pub struct FreezeContext {
    pub settings: Vec<SettingFileHashDto>,
    pub game_version: Option<String>,
    pub smapi_version: Option<String>,
}

/// The mods in a profile as they are now, sorted by UniqueID.
pub fn snapshot_mods(
    deployment_repo: &dyn DeploymentRepository,
    package_repo: &dyn PackageCatalogRepository,
    profile_id: &ProfileId,
) -> AppResult<Vec<FrozenModDto>> {
    let mut mods = Vec::new();
    for pc in deployment_repo.list_profile_components(profile_id)? {
        if let Some(component) = package_repo.get_package_component(&pc.package_component_id)? {
            mods.push(FrozenModDto {
                unique_id: component.unique_id.to_string(),
                name: component.name,
                version: component.version,
                artifact_hash: component.artifact_hash.to_string(),
                enabled: pc.enabled,
            });
        }
    }
    mods.sort_by_key(|a| a.unique_id.to_lowercase());
    Ok(mods)
}

pub struct ProfileFreeze {
    preferences: Arc<dyn PreferencesRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
}

impl ProfileFreeze {
    pub fn new(
        preferences: Arc<dyn PreferencesRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
    ) -> Self {
        Self {
            preferences,
            profile_repo,
            deployment_repo,
            package_repo,
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
            .map_err(|e| AppError::internal("Could not save the freeze", e.to_string()))?;
        self.preferences.set_preference(KEY, &json)
    }

    pub fn status(&self, profile_id: &ProfileId) -> AppResult<Option<ProfileFreezeDto>> {
        Ok(self
            .load()?
            .remove(&profile_id.to_string())
            .map(|stored| ProfileFreezeDto {
                profile_id: profile_id.to_string(),
                frozen_at: stored.frozen_at,
                reason: stored.reason,
                mods: stored.mods,
                settings: stored.settings,
                game_version: stored.game_version,
                smapi_version: stored.smapi_version,
            }))
    }

    /// Refuses a change to the mods installed in a frozen profile.
    pub fn ensure_not_frozen(&self, profile_id: &ProfileId) -> AppResult<()> {
        match self.status(profile_id)? {
            Some(freeze) => Err(AppError::validation(
                "PROFILE_FROZEN",
                format!(
                    "This profile is frozen{}. Unfreeze it on the Profiles page to install or remove mods.",
                    if freeze.reason.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", freeze.reason)
                    }
                ),
            )),
            None => Ok(()),
        }
    }

    pub fn freeze(&self, profile_id: &ProfileId, reason: &str) -> AppResult<ProfileFreezeDto> {
        self.freeze_with(profile_id, reason, FreezeContext::default())
    }

    /// Freezes with settings checksums and runtime observations recorded.
    pub fn freeze_with(
        &self,
        profile_id: &ProfileId,
        reason: &str,
        context: FreezeContext,
    ) -> AppResult<ProfileFreezeDto> {
        let reason = reason.trim().to_string();
        if reason.chars().count() > MAX_REASON_CHARS {
            return Err(AppError::validation(
                "FREEZE_REASON_TOO_LONG",
                format!("The reason can be at most {MAX_REASON_CHARS} characters"),
            ));
        }
        if self.profile_repo.get_profile(profile_id)?.is_none() {
            return Err(AppError::validation(
                "PROFILE_NOT_FOUND",
                "That profile does not exist",
            ));
        }
        let mods = snapshot_mods(&*self.deployment_repo, &*self.package_repo, profile_id)?;
        let stored = Stored {
            frozen_at: Utc::now().to_rfc3339(),
            reason,
            mods,
            settings: context.settings,
            game_version: context.game_version,
            smapi_version: context.smapi_version,
        };
        let mut map = self.load()?;
        map.insert(profile_id.to_string(), stored);
        self.save(&map)?;
        Ok(self.status(profile_id)?.expect("just saved"))
    }

    pub fn unfreeze(&self, profile_id: &ProfileId) -> AppResult<()> {
        let mut map = self.load()?;
        if map.remove(&profile_id.to_string()).is_some() {
            self.save(&map)?;
        }
        Ok(())
    }
}
