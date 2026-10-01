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
