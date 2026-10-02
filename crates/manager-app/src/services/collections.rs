//! Collections a user publishes from a profile.
//!
//! A draft is the curator's working state for a profile (name, notes and
//! per-mod choices) and can change freely. A published revision is a
//! recipe frozen under the collection's id and revision number: it is
//! never changed or replaced, so what revision 3 meant stays what it meant.

use crate::api::dto::CollectionRevisionDto;
use crate::error::{AppError, AppResult};
use crate::ports::repositories::PreferencesRepository;
use manager_core::ids::ProfileId;
use manager_core::recipe::ProfileRecipe;
use std::sync::Arc;

const MAX_DRAFT_BYTES: usize = 512 * 1024;
const MAX_REVISIONS: usize = 200;

fn draft_key(profile_id: &ProfileId) -> String {
    format!("collection_draft:{profile_id}")
}

fn revisions_key(collection_id: &str) -> String {
    format!("collection_revisions:{collection_id}")
}

pub struct Collections {
    preferences: Arc<dyn PreferencesRepository>,
}

impl Collections {
    pub fn new(preferences: Arc<dyn PreferencesRepository>) -> Self {
        Self { preferences }
    }

    /// The curator's draft for a profile, as the JSON the app saved.
    pub fn draft(&self, profile_id: &ProfileId) -> AppResult<Option<String>> {
        Ok(self
            .preferences
            .get_preference(&draft_key(profile_id))?
            .filter(|json| !json.is_empty()))
    }

    pub fn save_draft(&self, profile_id: &ProfileId, json: &str) -> AppResult<()> {
        if json.len() > MAX_DRAFT_BYTES {
            return Err(AppError::validation(
                "COLLECTION_DRAFT_TOO_LARGE",
                "The collection draft is too large",
            ));
        }
        let value: serde_json::Value = serde_json::from_str(json).map_err(|_| {
            AppError::validation(
                "COLLECTION_DRAFT_INVALID",
                "The collection draft is not valid",
            )
        })?;
        if !value.is_object() {
            return Err(AppError::validation(
                "COLLECTION_DRAFT_INVALID",
                "The collection draft is not valid",
            ));
        }
        self.preferences
            .set_preference(&draft_key(profile_id), json)
    }

    /// Every published revision of a collection, oldest first.
    pub fn revisions(&self, collection_id: &str) -> AppResult<Vec<CollectionRevisionDto>> {
        Ok(self
            .preferences
            .get_preference(&revisions_key(collection_id))?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    /// Publishes a recipe as the next revision of its collection. The recipe
    /// must name the collection and the next revision number; an existing
    /// revision is never overwritten.
    pub fn publish(&self, recipe_json: &str) -> AppResult<CollectionRevisionDto> {
        let recipe = ProfileRecipe::parse(recipe_json)
            .map_err(|errors| AppError::validation("RECIPE_INVALID", errors.join(" ")))?;
        let Some(collection) = &recipe.collection else {
            return Err(AppError::validation(
                "COLLECTION_MISSING",
                "Only a recipe that names its collection can be published",
            ));
        };
        let mut revisions = self.revisions(&collection.id)?;
        let next = revisions.last().map(|r| r.revision + 1).unwrap_or(1);
        if collection.revision != next {
            return Err(AppError::validation(
                "COLLECTION_REVISION_TAKEN",
                format!(
                    "Revision {} of this collection already exists or is out of order; the next is {next}",
                    collection.revision
                ),
            ));
        }
        if revisions.len() >= MAX_REVISIONS {
            return Err(AppError::validation(
                "COLLECTION_TOO_MANY_REVISIONS",
                "This collection has too many revisions to keep more",
            ));
        }
        let published = CollectionRevisionDto {
            collection_id: collection.id.clone(),
            revision: collection.revision,
            published_at: chrono::Utc::now().to_rfc3339(),
            recipe_json: recipe_json.to_string(),
        };
        revisions.push(published.clone());
        let json = serde_json::to_string(&revisions)
            .map_err(|e| AppError::internal("Could not save the revision", e.to_string()))?;
        self.preferences
            .set_preference(&revisions_key(&collection.id), &json)?;
        Ok(published)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::repositories::WindowGeometryDto;
    use manager_core::recipe::{CollectionInfo, RecipeGame};
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

    fn recipe(revision: u32) -> String {
        let mut r = ProfileRecipe::new(
            "Cozy",
            "2026-10-02T00:00:00Z",
            RecipeGame::default(),
            vec![],
        );
        r.collection = Some(CollectionInfo {
            id: "c1".into(),
            name: "Cozy".into(),
            author: "Me".into(),
            revision,
            notes: String::new(),
            forked_from: None,
        });
        r.to_json().unwrap()
    }

    #[test]
    fn revisions_are_published_in_order_and_never_replaced() {
        let store = Collections::new(Arc::new(Memory::default()));
        assert_eq!(store.publish(&recipe(1)).unwrap().revision, 1);
        assert_eq!(
            store.publish(&recipe(1)).unwrap_err().code,
            "COLLECTION_REVISION_TAKEN"
        );
        assert!(store.publish(&recipe(3)).is_err());
        store.publish(&recipe(2)).unwrap();
        let revisions = store.revisions("c1").unwrap();
        assert_eq!(
            revisions.iter().map(|r| r.revision).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(revisions[0].recipe_json.contains("\"revision\": 1"));
    }

    #[test]
    fn a_plain_recipe_is_not_a_collection_and_drafts_must_be_objects() {
        let store = Collections::new(Arc::new(Memory::default()));
        let plain = ProfileRecipe::new("x", "", RecipeGame::default(), vec![])
            .to_json()
            .unwrap();
        assert_eq!(
            store.publish(&plain).unwrap_err().code,
            "COLLECTION_MISSING"
        );
        let profile = ProfileId::new();
        assert!(store.save_draft(&profile, "[1]").is_err());
        store.save_draft(&profile, r#"{"name":"Cozy"}"#).unwrap();
        assert_eq!(
            store.draft(&profile).unwrap().as_deref(),
            Some(r#"{"name":"Cozy"}"#)
        );
    }
}
