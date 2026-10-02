//! Favourites, tags and notes the user keeps about mods.
//!
//! These are presentation data only: they are keyed by UniqueID (compared
//! case-insensitively, as SMAPI does), shared by every profile that has the mod,
//! and nothing that installs, enables or launches reads them.

use crate::api::dto::ModAnnotationDto;
use crate::error::{AppError, AppResult};
use crate::ports::repositories::PreferencesRepository;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

const KEY: &str = "mod_annotations";
pub const MAX_TAGS: usize = 20;
pub const MAX_TAG_CHARS: usize = 32;
pub const MAX_NOTE_CHARS: usize = 4000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Stored {
    unique_id: String,
    #[serde(default)]
    favourite: bool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    note: String,
    #[serde(default)]
    source_url: Option<String>,
    #[serde(default)]
    source_added_at: Option<String>,
}

pub struct ModAnnotations {
    preferences: Arc<dyn PreferencesRepository>,
}

/// Trims tags, collapses inner whitespace and drops case-insensitive repeats,
/// keeping the first spelling.
pub fn normalize_tags(tags: &[String]) -> AppResult<Vec<String>> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for tag in tags {
        let tag = tag.split_whitespace().collect::<Vec<_>>().join(" ");
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > MAX_TAG_CHARS {
            return Err(AppError::validation(
                "TAG_TOO_LONG",
                format!("Tags can be at most {MAX_TAG_CHARS} characters"),
            ));
        }
        if seen.insert(tag.to_lowercase()) {
            out.push(tag);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(AppError::validation(
            "TOO_MANY_TAGS",
            format!("A mod can have at most {MAX_TAGS} tags"),
        ));
    }
    Ok(out)
}

impl ModAnnotations {
    pub fn new(preferences: Arc<dyn PreferencesRepository>) -> Self {
        Self { preferences }
    }

    fn load(&self) -> AppResult<BTreeMap<String, Stored>> {
        // Unreadable data is dropped rather than breaking the Mods page: these
        // are the user's notes, not state anything depends on.
        Ok(self
            .preferences
            .get_preference(KEY)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    fn save(&self, map: &BTreeMap<String, Stored>) -> AppResult<()> {
        let json = serde_json::to_string(map)
            .map_err(|e| AppError::internal("Could not save mod notes", e.to_string()))?;
        self.preferences.set_preference(KEY, &json)
    }

    pub fn list(&self) -> AppResult<Vec<ModAnnotationDto>> {
        Ok(self
            .load()?
            .into_values()
            .map(|stored| ModAnnotationDto {
                unique_id: stored.unique_id,
                favourite: stored.favourite,
                tags: stored.tags,
                note: stored.note,
                source_url: stored.source_url,
                source_added_at: stored.source_added_at,
            })
            .collect())
    }

    /// Renames a tag on every mod (case-insensitively). Renaming to a tag a
    /// mod already has merges the two. Returns how many mods changed.
    pub fn rename_tag(&self, from: &str, to: &str) -> AppResult<usize> {
        let from = from.split_whitespace().collect::<Vec<_>>().join(" ");
        let renamed = normalize_tags(&[to.to_string()])?;
        let Some(to) = renamed.first().cloned() else {
            return Err(AppError::validation(
                "TAG_REQUIRED",
                "Give the tag a new name",
            ));
        };
        let mut map = self.load()?;
        let mut changed = 0;
        for stored in map.values_mut() {
            if !stored.tags.iter().any(|t| t.eq_ignore_ascii_case(&from)) {
                continue;
            }
            let tags: Vec<String> = stored
                .tags
                .iter()
                .map(|t| {
                    if t.eq_ignore_ascii_case(&from) {
                        to.clone()
                    } else {
                        t.clone()
                    }
                })
                .collect();
            stored.tags = normalize_tags(&tags)?;
            changed += 1;
        }
        if changed > 0 {
            self.save(&map)?;
        }
        Ok(changed)
    }

    /// Replaces everything stored for one mod. Clearing every field forgets it.
    pub fn set(
        &self,
        unique_id: &str,
        favourite: bool,
        tags: &[String],
        note: &str,
    ) -> AppResult<ModAnnotationDto> {
        let unique_id = unique_id.trim();
        if unique_id.is_empty() {
            return Err(AppError::validation(
                "MOD_ID_REQUIRED",
                "Notes can only be kept for a mod with a UniqueID",
            ));
        }
        let tags = normalize_tags(tags)?;
        let note = note.trim().to_string();
        if note.chars().count() > MAX_NOTE_CHARS {
            return Err(AppError::validation(
                "NOTE_TOO_LONG",
                format!("Notes can be at most {MAX_NOTE_CHARS} characters"),
            ));
        }
        let mut map = self.load()?;
        let key = unique_id.to_lowercase();
        // The source link is set on its own and kept when notes change.
        let (source_url, source_added_at) = map
            .get(&key)
            .map(|s| (s.source_url.clone(), s.source_added_at.clone()))
            .unwrap_or_default();
        if !favourite && tags.is_empty() && note.is_empty() && source_url.is_none() {
            map.remove(&key);
        } else {
            map.insert(
                key,
                Stored {
                    unique_id: unique_id.to_string(),
                    favourite,
                    tags: tags.clone(),
                    note: note.clone(),
                    source_url: source_url.clone(),
                    source_added_at: source_added_at.clone(),
                },
            );
        }
        self.save(&map)?;
        Ok(ModAnnotationDto {
            unique_id: unique_id.to_string(),
            favourite,
            tags,
            note,
            source_url,
            source_added_at,
        })
    }

    /// Sets or clears a source link the user supplies for a mod. Only web
    /// addresses are accepted; the mod's UniqueID, packages and acquisition
    /// records are not touched.
    pub fn set_source(&self, unique_id: &str, url: Option<&str>) -> AppResult<ModAnnotationDto> {
        let unique_id = unique_id.trim();
        if unique_id.is_empty() {
            return Err(AppError::validation(
                "MOD_ID_REQUIRED",
                "A source can only be kept for a mod with a UniqueID",
            ));
        }
        let url = url.map(str::trim).filter(|u| !u.is_empty());
        if let Some(url) = url {
            let lower = url.to_lowercase();
            if !(lower.starts_with("https://") || lower.starts_with("http://"))
                || url.contains(char::is_whitespace)
                || url.len() > 500
            {
                return Err(AppError::validation(
                    "SOURCE_URL_INVALID",
                    "Enter a web address starting with https:// or http://",
                ));
            }
        }
        let mut map = self.load()?;
        let key = unique_id.to_lowercase();
        let mut stored = map.remove(&key).unwrap_or(Stored {
            unique_id: unique_id.to_string(),
            favourite: false,
            tags: Vec::new(),
            note: String::new(),
            source_url: None,
            source_added_at: None,
        });
        stored.source_url = url.map(str::to_string);
        stored.source_added_at = url.map(|_| chrono::Utc::now().to_rfc3339());
        let dto = ModAnnotationDto {
            unique_id: stored.unique_id.clone(),
            favourite: stored.favourite,
            tags: stored.tags.clone(),
            note: stored.note.clone(),
            source_url: stored.source_url.clone(),
            source_added_at: stored.source_added_at.clone(),
        };
        if stored.favourite
            || !stored.tags.is_empty()
            || !stored.note.is_empty()
            || stored.source_url.is_some()
        {
            map.insert(key, stored);
        }
        self.save(&map)?;
        Ok(dto)
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;
    use crate::ports::repositories::WindowGeometryDto;
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

    #[test]
    fn a_source_link_is_kept_apart_from_notes_and_can_be_removed() {
        let notes = ModAnnotations::new(Arc::new(Memory::default()));
        assert!(notes.set_source("A.Mod", Some("ftp://x")).is_err());
        assert!(notes
            .set_source("A.Mod", Some("javascript:alert(1)"))
            .is_err());
        let set = notes
            .set_source("A.Mod", Some("https://forums.example.com/topic/1"))
            .unwrap();
        assert_eq!(
            set.source_url.as_deref(),
            Some("https://forums.example.com/topic/1")
        );
        assert!(set.source_added_at.is_some());
        // Changing notes keeps the link.
        let noted = notes.set("A.Mod", true, &[], "mine").unwrap();
        assert!(noted.source_url.is_some());
        // Clearing it removes only the link.
        let cleared = notes.set_source("a.mod", None).unwrap();
        assert!(cleared.source_url.is_none());
        assert!(cleared.favourite);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::repositories::WindowGeometryDto;
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

    fn service() -> ModAnnotations {
        ModAnnotations::new(Arc::new(Memory::default()))
    }

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn annotations_are_keyed_case_insensitively_and_replaced_whole() {
        let s = service();
        s.set(
            "Pathoschild.ContentPatcher",
            true,
            &tags(&["core"]),
            "Needed by most packs",
        )
        .unwrap();
        s.set(
            "pathoschild.contentpatcher",
            false,
            &tags(&["framework"]),
            "",
        )
        .unwrap();
        let all = s.list().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].unique_id, "pathoschild.contentpatcher");
        assert!(!all[0].favourite);
        assert_eq!(all[0].tags, vec!["framework"]);
        assert_eq!(all[0].note, "");
    }

    #[test]
    fn clearing_everything_forgets_the_mod() {
        let s = service();
        s.set("A.Mod", true, &[], "").unwrap();
        s.set("A.Mod", false, &tags(&["  "]), "   ").unwrap();
        assert!(s.list().unwrap().is_empty());
    }

    #[test]
    fn tags_are_tidied_and_limited() {
        assert_eq!(
            normalize_tags(&tags(&["  Visual  mods ", "visual mods", "UI", ""])).unwrap(),
            vec!["Visual mods", "UI"]
        );
        assert!(normalize_tags(&tags(&[&"x".repeat(MAX_TAG_CHARS + 1)])).is_err());
        let many: Vec<String> = (0..=MAX_TAGS).map(|i| format!("t{i}")).collect();
        assert!(normalize_tags(&many).is_err());
    }

    #[test]
    fn rejects_blank_ids_and_oversized_notes() {
        let s = service();
        assert!(s.set(" ", true, &[], "").is_err());
        assert!(s
            .set("A.Mod", false, &[], &"n".repeat(MAX_NOTE_CHARS + 1))
            .is_err());
    }

    #[test]
    fn unreadable_stored_data_is_treated_as_empty() {
        let memory = Arc::new(Memory::default());
        memory.set_preference(KEY, "not json").unwrap();
        assert!(ModAnnotations::new(memory).list().unwrap().is_empty());
    }

    #[test]
    fn renaming_a_tag_merges_it_everywhere() {
        let notes = ModAnnotations::new(Arc::new(Memory::default()));
        notes
            .set("A.Mod", false, &["Farm".into(), "Visual".into()], "")
            .unwrap();
        notes.set("B.Mod", true, &["farm".into()], "").unwrap();
        notes.set("C.Mod", false, &["Other".into()], "").unwrap();
        assert_eq!(notes.rename_tag("FARM", "Visual").unwrap(), 2);
        let mut all = notes.list().unwrap();
        all.sort_by(|a, b| a.unique_id.cmp(&b.unique_id));
        assert_eq!(all[0].tags, vec!["Visual".to_string()]);
        assert_eq!(all[1].tags, vec!["Visual".to_string()]);
        assert!(all[1].favourite);
        assert_eq!(all[2].tags, vec!["Other".to_string()]);
        assert_eq!(
            notes.rename_tag("Visual", " ").unwrap_err().code,
            "TAG_REQUIRED"
        );
    }
}
