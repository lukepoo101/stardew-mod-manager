//! The portable description of a profile.
//!
//! A recipe names what a profile contains by canonical identity, never by local
//! path, so it can move between people and operating systems. It is untrusted
//! input whenever it is read, so parsing is strict: a field of the wrong type is
//! an error, never a silent default.

use serde::{Deserialize, Serialize};

pub const RECIPE_SCHEMA: &str = "stardew-mod-manager.profile-recipe";
pub const RECIPE_SCHEMA_VERSION: u32 = 1;
const MAX_COMPONENTS: usize = 5000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeComponent {
    pub unique_id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub enabled: bool,
    /// SHA-256 of the package the exporter installed from, or empty if unknown.
    pub artifact_hash: String,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeGame {
    #[serde(default)]
    pub storefront: String,
    #[serde(default)]
    pub smapi_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileRecipe {
    pub schema: String,
    pub schema_version: u32,
    #[serde(default)]
    pub generated_at: String,
    pub profile_name: String,
    #[serde(default)]
    pub game: RecipeGame,
    pub components: Vec<RecipeComponent>,
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

impl ProfileRecipe {
    pub fn new(
        profile_name: impl Into<String>,
        generated_at: impl Into<String>,
        game: RecipeGame,
        mut components: Vec<RecipeComponent>,
    ) -> Self {
        // Sorted so an unchanged profile exports identically.
        components.sort_by(|a, b| {
            a.unique_id
                .cmp(&b.unique_id)
                .then_with(|| a.version.cmp(&b.version))
        });
        Self {
            schema: RECIPE_SCHEMA.to_string(),
            schema_version: RECIPE_SCHEMA_VERSION,
            generated_at: generated_at.into(),
            profile_name: profile_name.into(),
            game,
            components,
        }
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self)
            .map(|json| format!("{json}\n"))
            .map_err(|e| e.to_string())
    }

    /// Parses and validates untrusted recipe text, listing what is wrong.
    pub fn parse(text: &str) -> Result<Self, Vec<String>> {
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|_| vec!["The recipe is not valid JSON.".to_string()])?;
        let schema = value.get("schema").and_then(|v| v.as_str());
        if schema != Some(RECIPE_SCHEMA) {
            return Err(vec![
                "This is not a Stardew Mod Manager profile recipe.".to_string()
            ]);
        }
        let version = value.get("schema_version").and_then(|v| v.as_u64());
        if version != Some(RECIPE_SCHEMA_VERSION as u64) {
            return Err(vec![format!(
                "Unsupported recipe version {}; this manager reads version {}.",
                version.map(|v| v.to_string()).unwrap_or_else(|| "?".into()),
                RECIPE_SCHEMA_VERSION
            )]);
        }
        let recipe: ProfileRecipe = serde_json::from_value(value)
            .map_err(|e| vec![format!("The recipe is malformed: {e}")])?;

        let mut errors = Vec::new();
        if recipe.profile_name.trim().is_empty() {
            errors.push("profile_name must not be empty.".to_string());
        }
        if recipe.components.len() > MAX_COMPONENTS {
            return Err(vec!["The recipe lists too many components.".to_string()]);
        }
        for (index, component) in recipe.components.iter().enumerate() {
            if component.unique_id.trim().is_empty() {
                errors.push(format!("components[{index}].unique_id must not be empty."));
            }
            if !component.artifact_hash.is_empty() && !is_sha256(&component.artifact_hash) {
                errors.push(format!(
                    "components[{index}].artifact_hash is not a SHA-256 digest."
                ));
            }
        }
        if errors.is_empty() {
            Ok(recipe)
        } else {
            errors.truncate(10);
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(id: &str, version: &str, hash: &str) -> RecipeComponent {
        RecipeComponent {
            unique_id: id.into(),
            name: id.into(),
            author: "a".into(),
            version: version.into(),
            enabled: true,
            artifact_hash: hash.into(),
            optional: false,
        }
    }

    fn recipe() -> ProfileRecipe {
        ProfileRecipe::new(
            "Co-op",
            "2026-09-29T00:00:00Z",
            RecipeGame::default(),
            vec![
                component("B.Mod", "1.0", &"b".repeat(64)),
                component("A.Mod", "2.0", ""),
            ],
        )
    }

    #[test]
    fn a_recipe_round_trips_and_is_sorted_deterministically() {
        let json = recipe().to_json().unwrap();
        let parsed = ProfileRecipe::parse(&json).unwrap();
        assert_eq!(parsed, recipe());
        assert_eq!(parsed.components[0].unique_id, "A.Mod");
    }

    #[test]
    fn foreign_and_unsupported_documents_are_rejected_with_a_reason() {
        assert!(ProfileRecipe::parse("{").is_err());
        assert!(ProfileRecipe::parse("[]").is_err());
        let other = ProfileRecipe::parse(r#"{"schema":"other"}"#).unwrap_err();
        assert!(other[0].contains("not a Stardew Mod Manager"));
        let future = ProfileRecipe::parse(&format!(
            r#"{{"schema":"{RECIPE_SCHEMA}","schema_version":99}}"#
        ))
        .unwrap_err();
        assert!(future[0].contains("99"));
    }

    #[test]
    fn wrong_types_and_bad_hashes_are_errors_not_defaults() {
        let bad_type = format!(
            r#"{{"schema":"{RECIPE_SCHEMA}","schema_version":1,"profile_name":"x","components":[{{"unique_id":5}}]}}"#
        );
        assert!(ProfileRecipe::parse(&bad_type).is_err());

        let mut r = recipe();
        r.components[0].artifact_hash = "not-a-hash".into();
        let errors = ProfileRecipe::parse(&r.to_json().unwrap()).unwrap_err();
        assert!(errors[0].contains("SHA-256"));

        let mut r = recipe();
        r.components[0].unique_id = "  ".into();
        assert!(ProfileRecipe::parse(&r.to_json().unwrap()).is_err());
    }
}
