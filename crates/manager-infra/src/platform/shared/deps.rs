//! Reads the versions recorded in a .NET dependency manifest.
//!
//! The file format is the same on every platform, so the parsing is shared and
//! only the file it is read from is platform-specific.

use serde_json::Value;
use std::path::Path;

/// The version recorded for an assembly inside a deps.json target graph.
///
/// The manifest records one target per runtime identifier, and the assembly
/// version is the suffix of the assembly key. An unreadable or unrecognised
/// manifest yields no version rather than an error: a missing version is an
/// observation, not a failure.
pub fn assembly_version(path: &Path, assembly: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    version_from_deps_json(&text, assembly)
}

pub fn version_from_deps_json(text: &str, assembly: &str) -> Option<String> {
    let deps: Value = serde_json::from_str(text).ok()?;
    let prefix = format!("{}/", assembly);
    deps.get("targets")?
        .as_object()?
        .values()
        .filter_map(Value::as_object)
        .flat_map(|target| target.keys())
        .find_map(|key| key.strip_prefix(&prefix).map(str::to_owned))
}

/// The installed Stardew Valley version, read from Stardew Valley.deps.json.
pub fn game_version(game_dir: &Path) -> Option<String> {
    assembly_version(&game_dir.join("Stardew Valley.deps.json"), "Stardew Valley")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_read_from_the_first_target_that_records_the_assembly() {
        let text = r#"{"targets":{"net6":{"Stardew Valley/1.6.15.24356":{}}}}"#;
        assert_eq!(
            version_from_deps_json(text, "Stardew Valley").as_deref(),
            Some("1.6.15.24356")
        );
        assert_eq!(version_from_deps_json(text, "StardewModdingAPI"), None);
    }

    #[test]
    fn an_unrecognised_manifest_does_not_invent_a_version() {
        assert_eq!(version_from_deps_json("not json", "Stardew Valley"), None);
        assert_eq!(version_from_deps_json("{}", "Stardew Valley"), None);
    }
}
