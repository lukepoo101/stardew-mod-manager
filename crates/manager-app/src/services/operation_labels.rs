//! Which larger change an operation was part of.
//!
//! A reinstall, a version change or a restore is made of several journaled
//! operations (removals, then installs). Each is recorded on its own; this
//! notes the change they belong to so history can show them together. It is
//! descriptive only: nothing decides anything from it.

use crate::error::{AppError, AppResult};
use crate::ports::repositories::PreferencesRepository;
use manager_core::ids::OperationId;
use std::collections::HashMap;

const KEY: &str = "operation_labels";
/// Labels kept, newest last; older ones are dropped.
const MAX_LABELS: usize = 1000;

fn load(preferences: &dyn PreferencesRepository) -> AppResult<Vec<(String, String)>> {
    Ok(preferences
        .get_preference(KEY)?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

/// Notes that `operation` was part of the change described by `label`.
pub fn label_operation(
    preferences: &dyn PreferencesRepository,
    operation: &OperationId,
    label: &str,
) -> AppResult<()> {
    let mut labels = load(preferences)?;
    let id = operation.to_string();
    labels.retain(|(existing, _)| existing != &id);
    labels.push((id, label.to_string()));
    if labels.len() > MAX_LABELS {
        let excess = labels.len() - MAX_LABELS;
        labels.drain(..excess);
    }
    let json = serde_json::to_string(&labels)
        .map_err(|e| AppError::internal("Could not save operation labels", e.to_string()))?;
    preferences.set_preference(KEY, &json)
}

/// Every recorded label by operation id.
pub fn operation_labels(
    preferences: &dyn PreferencesRepository,
) -> AppResult<HashMap<String, String>> {
    Ok(load(preferences)?.into_iter().collect())
}

/// What an operation of this kind does, as a short phrase for messages such
/// as "Another change is in progress (installing a mod)".
pub fn kind_phrase(kind: manager_core::operation::OperationKind) -> &'static str {
    use manager_core::operation::OperationKind;
    match kind {
        OperationKind::SmapiSetup => "setting up SMAPI",
        OperationKind::ModInstall => "installing a mod",
        OperationKind::ModRemove => "removing a mod",
        OperationKind::GameLaunch => "starting the game",
        OperationKind::ProfileCreate => "creating a profile",
        OperationKind::ProfileDelete => "deleting a profile",
        OperationKind::ModFilesAccepted => "accepting changed mod files",
        OperationKind::ModSettingsApplied => "applying shared mod settings",
        OperationKind::ProfileChangeSet => "a change made of several steps",
        OperationKind::ModToggle => "turning mods on or off",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_plain_phrase() {
        use manager_core::operation::OperationKind;
        for kind in [
            OperationKind::SmapiSetup,
            OperationKind::ModInstall,
            OperationKind::ModRemove,
            OperationKind::GameLaunch,
            OperationKind::ProfileCreate,
            OperationKind::ProfileDelete,
            OperationKind::ModFilesAccepted,
            OperationKind::ModSettingsApplied,
            OperationKind::ProfileChangeSet,
            OperationKind::ModToggle,
        ] {
            let phrase = kind_phrase(kind);
            assert!(!phrase.is_empty() && !phrase.contains('_'), "{phrase}");
        }
    }
}
