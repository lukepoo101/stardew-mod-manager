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
    /// "exact" (the default) or "at_least": whether a newer version also
    /// satisfies the requirement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_rule: Option<String>,
    /// The optional group this component belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Where to get a component the recipient must download themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<ManualSource>,
    /// Only affects the player's own computer (visual mods, for example),
    /// so players need not match it in multiplayer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub client_only: bool,
    /// The curator's reason for including it, shown as their words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Settings files the curator chose to share for this mod, by path in
    /// the mod's folder. Only `config.json` files, as text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<RecipeSetting>,
    /// Where the author says the mod is published (manifest UpdateKeys such
    /// as "Nexus:1915"), as declared.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub update_keys: Vec<String>,
    /// A web page for the mod that the exporter added themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// UniqueIDs its manifest requires, so recipients see what an optional
    /// mod brings with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
}

/// One shared settings file: its path in the mod folder, its text and the
/// SHA-256 of that text, so a recipient can tell whether theirs matches
/// without the values being compared by eye.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeSetting {
    pub path: String,
    pub sha256: String,
    pub content: String,
}

/// The largest settings file a recipe may carry.
pub const MAX_SETTING_BYTES: usize = 256 * 1024;

impl RecipeSetting {
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        let content = content.into();
        Self {
            path: path.into(),
            sha256: settings_sha256(content.as_bytes()),
            content,
        }
    }

    /// Why this entry cannot be used, if it cannot: the path must be a plain
    /// relative path to a `config.json`, the text small enough, and the
    /// checksum must match the text.
    pub fn problem(&self) -> Option<String> {
        let plain = !self.path.is_empty()
            && !self.path.starts_with('/')
            && !self.path.contains('\\')
            && self
                .path
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..");
        if !plain {
            return Some(format!("{} is not a plain relative path", self.path));
        }
        let is_config = self
            .path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("config.json"));
        if !is_config {
            return Some(format!("{} is not a config.json file", self.path));
        }
        if self.content.len() > MAX_SETTING_BYTES {
            return Some(format!("{} is too large to share", self.path));
        }
        if !self
            .sha256
            .eq_ignore_ascii_case(&settings_sha256(self.content.as_bytes()))
            // Recipes made before line endings were normalised.
            && !self
                .sha256
                .eq_ignore_ascii_case(&sha256_hex(self.content.as_bytes()))
        {
            return Some(format!("{} does not match its checksum", self.path));
        }
        None
    }
}

/// The checksum used for settings files: SHA-256 of the text with Windows
/// line endings (CRLF) read as LF, so the same settings compare equal on
/// every operating system.
pub fn settings_sha256(bytes: &[u8]) -> String {
    let mut normalised = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter().peekable();
    while let Some(&b) = iter.next() {
        if b == b'\r' && iter.peek() == Some(&&b'\n') {
            continue;
        }
        normalised.push(b);
    }
    sha256_hex(&normalised)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A requirement fetched by hand, with where and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualSource {
    pub url: String,
    #[serde(default)]
    pub instructions: String,
}

/// Who publishes a collection and which revision this is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    pub revision: u32,
    #[serde(default)]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<CollectionLineage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionLineage {
    pub id: String,
    pub revision: u32,
    #[serde(default)]
    pub name: String,
}

/// A named set of optional components the recipient chooses from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionGroup {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// "any" (choose any number) or "one" (choose one).
    #[serde(default = "any_choice")]
    pub choose: String,
}

fn any_choice() -> String {
    "any".to_string()
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
    /// Present when the recipe is a published collection revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection: Option<CollectionInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<OptionGroup>,
    /// Set when the profile was frozen at these versions when shared, for
    /// example a multiplayer group's agreed setup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen: Option<FrozenInfo>,
    /// Mods the exporter's own reference asks for that were not installed
    /// when this was exported, so the profile it describes is incomplete.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incomplete: Vec<IncompleteItem>,
}

/// A mod missing from the exported profile, as its reference names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncompleteItem {
    pub unique_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub artifact_hash: String,
}

/// When and why the shared setup was frozen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenInfo {
    pub frozen_at: String,
    #[serde(default)]
    pub reason: String,
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
            collection: None,
            groups: Vec::new(),
            frozen: None,
            incomplete: Vec::new(),
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
            if let Some(rule) = &component.version_rule {
                if rule != "exact" && rule != "at_least" {
                    errors.push(format!(
                        "components[{index}].version_rule must be exact or at_least."
                    ));
                }
            }
            if let Some(url) = &component.source_url {
                let lower = url.to_lowercase();
                if !(lower.starts_with("https://") || lower.starts_with("http://")) {
                    errors.push(format!(
                        "components[{index}].source_url must be a web address."
                    ));
                }
            }
            if component.update_keys.len() > 20
                || component.update_keys.iter().any(|k| k.len() > 200)
                || component.requires.len() > 200
            {
                errors.push(format!(
                    "components[{index}] lists too many update keys or requirements."
                ));
            }
            for setting in &component.settings {
                if let Some(problem) = setting.problem() {
                    errors.push(format!("components[{index}].settings: {problem}."));
                }
            }
            if let Some(manual) = &component.manual {
                let url = manual.url.to_lowercase();
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    errors.push(format!(
                        "components[{index}].manual.url must be a web address."
                    ));
                }
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
            version_rule: None,
            group: None,
            manual: None,
            client_only: false,
            note: None,
            settings: Vec::new(),
            update_keys: Vec::new(),
            source_url: None,
            requires: Vec::new(),
        }
    }

    #[test]
    fn shared_settings_are_checked_against_their_checksum() {
        let mut r = recipe();
        r.components[0].settings = vec![RecipeSetting::new("config.json", "{\"a\":1}")];
        let parsed = ProfileRecipe::parse(&r.to_json().unwrap()).unwrap();
        assert_eq!(parsed.components[0].settings[0].content, "{\"a\":1}");

        r.components[0].settings[0].content = "{\"a\":2}".into();
        assert!(ProfileRecipe::parse(&r.to_json().unwrap())
            .unwrap_err()
            .join(" ")
            .contains("does not match its checksum"));
        for path in [
            "../config.json",
            "/config.json",
            "data/other.json",
            "a//config.json",
        ] {
            let setting = RecipeSetting::new(path, "{}");
            assert!(setting.problem().is_some(), "{path}");
        }
        assert!(RecipeSetting::new("assets/config.json", "{}")
            .problem()
            .is_none());
        // Line endings do not change the checksum.
        assert_eq!(
            RecipeSetting::new("config.json", "{\r\n}").sha256,
            RecipeSetting::new("config.json", "{\n}").sha256
        );
    }

    #[test]
    fn collection_fields_round_trip_and_are_validated() {
        let mut r = recipe();
        r.collection = Some(CollectionInfo {
            id: "c1".into(),
            name: "Cozy".into(),
            author: "Me".into(),
            revision: 2,
            notes: "Fixes".into(),
            forked_from: None,
        });
        r.groups.push(OptionGroup {
            name: "Portraits".into(),
            description: "Pick one".into(),
            choose: "one".into(),
        });
        r.components[0].version_rule = Some("at_least".into());
        r.components[0].group = Some("Portraits".into());
        r.components[1].manual = Some(ManualSource {
            url: "https://forums.example.com/1".into(),
            instructions: "Download the zip".into(),
        });
        let parsed = ProfileRecipe::parse(&r.to_json().unwrap()).unwrap();
        assert_eq!(parsed, r);

        r.components[0].version_rule = Some("roughly".into());
        assert!(ProfileRecipe::parse(&r.to_json().unwrap()).is_err());
        r.components[0].version_rule = None;
        r.components[1].manual.as_mut().unwrap().url = "javascript:x".into();
        assert!(ProfileRecipe::parse(&r.to_json().unwrap()).is_err());
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
