//! A recipe a profile keeps as its reference, such as a multiplayer group's
//! shared setup, and the differences the user accepted for that group.
//!
//! The reference is only ever compared with; nothing here installs, removes or
//! changes mods. It is validated with the same strict parser as any recipe.

use crate::api::dto::ReferenceRecipeDto;
use crate::error::{AppError, AppResult};
use crate::ports::repositories::PreferencesRepository;
use chrono::Utc;
use manager_core::ids::ProfileId;
use manager_core::recipe::ProfileRecipe;
use std::sync::Arc;

fn key(profile_id: &ProfileId) -> String {
    format!("reference_recipe:{profile_id}")
}

pub struct ReferenceRecipes {
    preferences: Arc<dyn PreferencesRepository>,
}

impl ReferenceRecipes {
    pub fn new(preferences: Arc<dyn PreferencesRepository>) -> Self {
        Self { preferences }
    }

    pub fn get(&self, profile_id: &ProfileId) -> AppResult<Option<ReferenceRecipeDto>> {
        Ok(self
            .preferences
            .get_preference(&key(profile_id))?
            .filter(|json| !json.is_empty())
            .and_then(|json| serde_json::from_str(&json).ok()))
    }

    fn save(&self, profile_id: &ProfileId, value: &ReferenceRecipeDto) -> AppResult<()> {
        let json = serde_json::to_string(value)
            .map_err(|e| AppError::internal("Could not save the reference", e.to_string()))?;
        self.preferences.set_preference(&key(profile_id), &json)
    }

    /// Keeps a recipe as the profile's reference, replacing any earlier one
    /// and its accepted differences.
    pub fn attach(
        &self,
        profile_id: &ProfileId,
        recipe_json: &str,
    ) -> AppResult<ReferenceRecipeDto> {
        ProfileRecipe::parse(recipe_json)
            .map_err(|errors| AppError::validation("RECIPE_INVALID", errors.join(" ")))?;
        let value = ReferenceRecipeDto {
            recipe_json: recipe_json.to_string(),
            attached_at: Utc::now().to_rfc3339(),
            accepted: Vec::new(),
            accepted_notes: Default::default(),
        };
        self.save(profile_id, &value)?;
        Ok(value)
    }

    pub fn set_accepted(
        &self,
        profile_id: &ProfileId,
        difference_key: &str,
        accepted: bool,
    ) -> AppResult<ReferenceRecipeDto> {
        self.set_accepted_with_note(profile_id, difference_key, accepted, None)
    }

    /// As [`Self::set_accepted`], keeping why the difference is fine.
    pub fn set_accepted_with_note(
        &self,
        profile_id: &ProfileId,
        difference_key: &str,
        accepted: bool,
        note: Option<&str>,
    ) -> AppResult<ReferenceRecipeDto> {
        let mut value = self.get(profile_id)?.ok_or_else(|| {
            AppError::validation("NO_REFERENCE", "This profile has no reference recipe")
        })?;
        value.accepted.retain(|k| k != difference_key);
        value.accepted_notes.remove(difference_key);
        if accepted {
            value.accepted.push(difference_key.to_string());
            value.accepted.sort();
            if let Some(note) = note.map(str::trim).filter(|n| !n.is_empty()) {
                value
                    .accepted_notes
                    .insert(difference_key.to_string(), note.chars().take(300).collect());
            }
        }
        self.save(profile_id, &value)?;
        Ok(value)
    }

    pub fn detach(&self, profile_id: &ProfileId) -> AppResult<()> {
        self.preferences.set_preference(&key(profile_id), "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::repositories::WindowGeometryDto;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Memory(Mutex<BTreeMap<String, String>>);

    impl PreferencesRepository for Memory {
        fn get_window_geometry(&self) -> AppResult<Option<WindowGeometryDto>> {
            Ok(None)
        }
        fn save_window_geometry(&self, _: &WindowGeometryDto) -> AppResult<()> {
            Ok(())
        }
        fn get_preference(&self, key: &str) -> AppResult<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set_preference(&self, key: &str, value: &str) -> AppResult<()> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
    }

    fn recipe() -> String {
        manager_core::recipe::ProfileRecipe::new(
            "Group",
            "2026-09-01T00:00:00Z",
            manager_core::recipe::RecipeGame {
                storefront: "Steam".into(),
                smapi_version: None,
            },
            Vec::new(),
        )
        .to_json()
        .unwrap()
    }

    #[test]
    fn a_reference_is_validated_kept_and_can_be_removed() {
        let refs = ReferenceRecipes::new(Arc::new(Memory::default()));
        let profile = ProfileId::new();
        assert!(refs.get(&profile).unwrap().is_none());
        assert!(refs.attach(&profile, "{\"not\":\"a recipe\"}").is_err());
        assert!(refs.get(&profile).unwrap().is_none());

        refs.attach(&profile, &recipe()).unwrap();
        refs.set_accepted(&profile, "version:A.Mod:1.0:1.1", true)
            .unwrap();
        refs.set_accepted(&profile, "version:A.Mod:1.0:1.1", true)
            .unwrap();
        assert_eq!(refs.get(&profile).unwrap().unwrap().accepted.len(), 1);
        refs.set_accepted(&profile, "version:A.Mod:1.0:1.1", false)
            .unwrap();
        assert!(refs.get(&profile).unwrap().unwrap().accepted.is_empty());

        refs.set_accepted(&profile, "extra:B.Mod::2.0", true)
            .unwrap();
        // A new reference starts with nothing accepted.
        let fresh = refs.attach(&profile, &recipe()).unwrap();
        assert!(fresh.accepted.is_empty());

        refs.detach(&profile).unwrap();
        assert!(refs.get(&profile).unwrap().is_none());
        assert!(refs.set_accepted(&profile, "x", true).is_err());
    }
}
