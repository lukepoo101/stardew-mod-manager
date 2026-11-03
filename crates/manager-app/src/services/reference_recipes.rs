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

/// Keys for a missing mod used to name only versions
/// (`missing:<id>:<version>:`); they now name the package too
/// (`missing:<id>:<version>::<hash>:`), so a mod republished as another file
/// at the same version is a new difference. An older key is read as the
/// mod's package in the reference it was accepted against. An older
/// "different package" key cannot be mapped (the installed file is not
/// recorded), so that difference is shown once more.
fn with_current_keys(mut value: ReferenceRecipeDto) -> ReferenceRecipeDto {
    let Ok(recipe) = ProfileRecipe::parse(&value.recipe_json) else {
        return value;
    };
    let current = |key: &str| -> Option<String> {
        let rest = key.strip_prefix("missing:")?;
        let (id, rest) = rest.split_once(':')?;
        let version = rest.strip_suffix(':')?;
        if version.contains(':') {
            return None;
        }
        let component = recipe
            .components
            .iter()
            .find(|c| c.unique_id == id && c.version == version)?;
        Some(format!(
            "missing:{id}:{version}::{}:",
            component.artifact_hash.to_lowercase()
        ))
    };
    let mut accepted = Vec::new();
    for key in std::mem::take(&mut value.accepted) {
        match current(&key) {
            Some(mapped) => {
                if let Some(note) = value.accepted_notes.remove(&key) {
                    value.accepted_notes.insert(mapped.clone(), note);
                }
                accepted.push(mapped);
            }
            None => accepted.push(key),
        }
    }
    accepted.sort();
    accepted.dedup();
    value.accepted = accepted;
    value
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
            .and_then(|json| serde_json::from_str(&json).ok())
            .map(with_current_keys))
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
        self.attach_keeping(profile_id, recipe_json, &[])
    }

    /// As [`Self::attach`], keeping the accepted differences (and the reasons
    /// given) that `still_apply` names: the differences the profile has from
    /// the new recipe. A key names both sides, so a difference whose sides
    /// are unchanged is the same difference and stays accepted; anything
    /// that changed is shown again.
    pub fn attach_keeping(
        &self,
        profile_id: &ProfileId,
        recipe_json: &str,
        still_apply: &[String],
    ) -> AppResult<ReferenceRecipeDto> {
        ProfileRecipe::parse(recipe_json)
            .map_err(|errors| AppError::validation("RECIPE_INVALID", errors.join(" ")))?;
        let previous = self.get(profile_id)?;
        let mut accepted = Vec::new();
        let mut accepted_notes = std::collections::BTreeMap::new();
        if let Some(previous) = previous {
            for key in previous.accepted {
                if still_apply.contains(&key) {
                    if let Some(note) = previous.accepted_notes.get(&key) {
                        accepted_notes.insert(key.clone(), note.clone());
                    }
                    accepted.push(key);
                }
            }
        }
        accepted.sort();
        accepted.dedup();
        let value = ReferenceRecipeDto {
            recipe_json: recipe_json.to_string(),
            attached_at: Utc::now().to_rfc3339(),
            accepted,
            accepted_notes,
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

    #[test]
    fn an_older_missing_key_is_read_as_naming_the_package() {
        let refs = ReferenceRecipes::new(Arc::new(Memory::default()));
        let profile = ProfileId::new();
        let component = |id: &str| manager_core::recipe::RecipeComponent {
            unique_id: id.into(),
            name: id.into(),
            author: "a".into(),
            version: "1.0.0".into(),
            enabled: true,
            artifact_hash: "AB".repeat(32),
            optional: true,
            version_rule: None,
            group: None,
            manual: None,
            client_only: false,
            note: None,
            settings: Vec::new(),
            update_keys: Vec::new(),
            source_url: None,
            requires: Vec::new(),
            locally_modified: false,
            platforms: Vec::new(),
        };
        let json = manager_core::recipe::ProfileRecipe::new(
            "Group",
            "2026-09-01T00:00:00Z",
            manager_core::recipe::RecipeGame::default(),
            vec![component("Opt.Mod")],
        )
        .to_json()
        .unwrap();
        refs.attach(&profile, &json).unwrap();
        refs.set_accepted_with_note(&profile, "missing:Opt.Mod:1.0.0:", true, Some("not for me"))
            .unwrap();
        let read = refs.get(&profile).unwrap().unwrap();
        let mapped = format!("missing:Opt.Mod:1.0.0::{}:", "ab".repeat(32));
        assert_eq!(read.accepted, vec![mapped.clone()]);
        assert_eq!(
            read.accepted_notes.get(&mapped).map(String::as_str),
            Some("not for me")
        );
    }

    #[test]
    fn a_new_reference_keeps_the_acceptances_that_still_apply_with_their_reasons() {
        let refs = ReferenceRecipes::new(Arc::new(Memory::default()));
        let profile = ProfileId::new();
        refs.attach(&profile, &recipe()).unwrap();
        let same = "enabled:CJBok.CheatsMenu:1.37.0:1.37.0".to_string();
        let changed = "version:Pathoschild.LookupAnything:1.49.0:1.48.0".to_string();
        let own = "extra:Guest.PetHats::1.0.0".to_string();
        refs.set_accepted_with_note(&profile, &same, true, None)
            .unwrap();
        refs.set_accepted_with_note(&profile, &changed, true, Some("testing a beta"))
            .unwrap();
        refs.set_accepted_with_note(&profile, &own, true, Some("my own cosmetic"))
            .unwrap();

        // The new recipe moved Lookup Anything to 1.50.0, so that difference
        // is a new one; the other two are unchanged.
        let still = vec![
            same.clone(),
            own.clone(),
            "version:Pathoschild.LookupAnything:1.50.0:1.48.0".to_string(),
        ];
        let kept = refs.attach_keeping(&profile, &recipe(), &still).unwrap();
        assert_eq!(kept.accepted, vec![same, own.clone()]);
        assert_eq!(
            kept.accepted_notes.get(&own).map(String::as_str),
            Some("my own cosmetic")
        );
        assert!(!kept.accepted_notes.contains_key(&changed));
        assert_eq!(refs.get(&profile).unwrap().unwrap().accepted, kept.accepted);
    }
}
