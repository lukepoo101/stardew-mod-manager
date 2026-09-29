use serde::{Deserialize, Serialize};

/// The game and SMAPI versions observed at one moment.
///
/// A missing value means "not observed", never "unchanged": comparisons treat
/// unknown as unknown so a version that could not be read can neither raise nor
/// clear a warning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeVersions {
    pub game_version: Option<String>,
    pub smapi_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeChange {
    Game { before: String, now: String },
    Smapi { before: String, now: String },
}

/// What changed between the versions a profile last worked with and now.
///
/// Only a difference between two *known* values is reported.
pub fn runtime_changes(before: &RuntimeVersions, now: &RuntimeVersions) -> Vec<RuntimeChange> {
    let mut changes = Vec::new();
    if let (Some(a), Some(b)) = (&before.game_version, &now.game_version) {
        if a != b {
            changes.push(RuntimeChange::Game {
                before: a.clone(),
                now: b.clone(),
            });
        }
    }
    if let (Some(a), Some(b)) = (&before.smapi_version, &now.smapi_version) {
        if a != b {
            changes.push(RuntimeChange::Smapi {
                before: a.clone(),
                now: b.clone(),
            });
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(game: Option<&str>, smapi: Option<&str>) -> RuntimeVersions {
        RuntimeVersions {
            game_version: game.map(str::to_string),
            smapi_version: smapi.map(str::to_string),
        }
    }

    #[test]
    fn identical_versions_report_nothing() {
        let a = v(Some("1.6.15"), Some("4.1.10"));
        assert!(runtime_changes(&a, &a.clone()).is_empty());
    }

    #[test]
    fn each_changed_component_is_reported_separately() {
        let changes = runtime_changes(
            &v(Some("1.6.14"), Some("4.1.9")),
            &v(Some("1.6.15"), Some("4.1.10")),
        );
        assert_eq!(changes.len(), 2);
        assert!(matches!(changes[0], RuntimeChange::Game { .. }));
        assert!(matches!(changes[1], RuntimeChange::Smapi { .. }));
    }

    #[test]
    fn an_unobserved_version_is_never_a_change() {
        assert!(runtime_changes(&v(Some("1"), None), &v(None, Some("2"))).is_empty());
        assert!(runtime_changes(&v(None, None), &v(Some("1"), Some("2"))).is_empty());
    }
}
