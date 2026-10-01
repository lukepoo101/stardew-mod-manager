//! Stardew Valley saves: which profile each usually goes with, and backups.
//!
//! Associations are the user's choice, keyed by the save folder's name (which
//! the game never changes) and by profile id (which renaming does not change).
//! They never touch the save. Backups and restores are separate, explicit
//! actions, independent of mod and profile recovery, and are refused while the
//! game is running so a save being written is never copied half way.

use crate::api::dto::{SaveBackupDto, SaveDto, SaveLinkDto, SavesDto};
use crate::error::{AppError, AppResult};
use crate::ports::launcher::GameLauncherPort;
use crate::ports::repositories::{PreferencesRepository, ProfileRepository};
use crate::ports::saves::{SaveBackupInfo, SavesPort};
use manager_core::ids::ProfileId;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

const KEY: &str = "save_associations";

pub struct SavesService {
    saves: Arc<dyn SavesPort>,
    preferences: Arc<dyn PreferencesRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
    launcher: Arc<dyn GameLauncherPort>,
}

fn backup_dto(b: SaveBackupInfo) -> SaveBackupDto {
    SaveBackupDto {
        id: b.id,
        save_id: b.save_id,
        created_at: b.created_at.to_rfc3339(),
        size_bytes: b.size_bytes,
    }
}

impl SavesService {
    pub fn new(
        saves: Arc<dyn SavesPort>,
        preferences: Arc<dyn PreferencesRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
        launcher: Arc<dyn GameLauncherPort>,
    ) -> Self {
        Self {
            saves,
            preferences,
            profile_repo,
            launcher,
        }
    }

    fn associations(&self) -> AppResult<BTreeMap<String, String>> {
        Ok(self
            .preferences
            .get_preference(KEY)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    fn refuse_while_playing(&self) -> AppResult<()> {
        if self.launcher.is_game_running(None) {
            return Err(AppError::game_running(
                "Close Stardew Valley first, so the save is not copied while it is being written",
            ));
        }
        Ok(())
    }

    pub fn list(&self) -> AppResult<SavesDto> {
        let associations = self.associations()?;
        let mut backups: BTreeMap<String, Vec<SaveBackupDto>> = BTreeMap::new();
        for backup in self.saves.list_backups()? {
            backups
                .entry(backup.save_id.clone())
                .or_default()
                .push(backup_dto(backup));
        }
        let mut saves = Vec::new();
        for save in self.saves.list_saves()? {
            let profile_id = associations.get(&save.id).cloned();
            let profile_name = match &profile_id {
                Some(id) => match ProfileId::from_str(id) {
                    Ok(pid) => self.profile_repo.get_profile(&pid)?.map(|p| p.name),
                    Err(_) => None,
                },
                None => None,
            };
            saves.push(SaveDto {
                backups: backups.remove(&save.id).unwrap_or_default(),
                id: save.id,
                farm_name: save.farm_name,
                farmer_name: save.farmer_name,
                game_version: save.game_version,
                modified_at: save.modified_at.map(|t| t.to_rfc3339()),
                size_bytes: save.size_bytes,
                profile_id,
                profile_name,
            });
        }
        let found: std::collections::HashSet<&str> = saves.iter().map(|s| s.id.as_str()).collect();
        let mut unavailable_links = Vec::new();
        for (save_id, profile_id) in &associations {
            if found.contains(save_id.as_str()) {
                continue;
            }
            let profile_name = match ProfileId::from_str(profile_id) {
                Ok(pid) => self.profile_repo.get_profile(&pid)?.map(|p| p.name),
                Err(_) => None,
            };
            unavailable_links.push(SaveLinkDto {
                save_id: save_id.clone(),
                profile_id: profile_id.clone(),
                profile_name,
            });
        }
        Ok(SavesDto {
            saves_dir: self.saves.saves_dir().map(|p| p.display().to_string()),
            saves,
            unavailable_links,
        })
    }

    /// Links a save to a profile, or removes the link with `None`.
    pub fn associate(&self, save_id: &str, profile_id: Option<&ProfileId>) -> AppResult<()> {
        // A link to a save that has since gone can still be removed.
        if profile_id.is_some() && !self.saves.list_saves()?.iter().any(|s| s.id == save_id) {
            return Err(AppError::validation(
                "SAVE_NOT_FOUND",
                "That save is not in the Stardew Valley save folder",
            ));
        }
        let mut map = self.associations()?;
        match profile_id {
            Some(pid) => {
                if self.profile_repo.get_profile(pid)?.is_none() {
                    return Err(AppError::validation(
                        "PROFILE_NOT_FOUND",
                        "That profile does not exist",
                    ));
                }
                map.insert(save_id.to_string(), pid.to_string());
            }
            None => {
                map.remove(save_id);
            }
        }
        let json = serde_json::to_string(&map)
            .map_err(|e| AppError::internal("Could not save the link", e.to_string()))?;
        self.preferences.set_preference(KEY, &json)
    }

    pub fn backup(&self, save_id: &str) -> AppResult<SaveBackupDto> {
        self.refuse_while_playing()?;
        self.saves.backup(save_id).map(backup_dto)
    }

    /// Restores a backup; the live save is backed up first. Mods and profiles
    /// are not changed.
    pub fn restore(&self, backup_id: &str) -> AppResult<SaveBackupDto> {
        self.refuse_while_playing()?;
        self.saves.restore(backup_id).map(backup_dto)
    }
}
