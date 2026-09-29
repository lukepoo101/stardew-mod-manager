//! Findings the user has chosen not to be reminded about.
//!
//! A dismissal hides one finding, not the condition behind it: it records the
//! finding's signature (a digest of what it currently says), and the finding
//! comes back on its own as soon as that changes. Serious findings can never be
//! dismissed.

use crate::api::dto::DismissedFindingDto;
use crate::error::{AppError, AppResult};
use crate::ports::repositories::PreferencesRepository;
use std::collections::BTreeMap;
use std::sync::Arc;

const KEY: &str = "dismissed_findings";

pub struct FindingDismissals {
    preferences: Arc<dyn PreferencesRepository>,
}

/// Severities that must stay visible whatever the user prefers.
fn is_dismissable(severity: &str) -> bool {
    matches!(
        severity.trim().to_lowercase().as_str(),
        "info" | "recommendation" | "warning"
    )
}

impl FindingDismissals {
    pub fn new(preferences: Arc<dyn PreferencesRepository>) -> Self {
        Self { preferences }
    }

    fn load(&self) -> AppResult<BTreeMap<String, String>> {
        Ok(self
            .preferences
            .get_preference(KEY)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    fn save(&self, map: &BTreeMap<String, String>) -> AppResult<()> {
        let json = serde_json::to_string(map)
            .map_err(|e| AppError::internal("Could not save dismissals", e.to_string()))?;
        self.preferences.set_preference(KEY, &json)
    }

    pub fn list(&self) -> AppResult<Vec<DismissedFindingDto>> {
        Ok(self
            .load()?
            .into_iter()
            .map(|(fingerprint, signature)| DismissedFindingDto {
                fingerprint,
                signature,
            })
            .collect())
    }

    pub fn dismiss(&self, fingerprint: &str, signature: &str, severity: &str) -> AppResult<()> {
        if !is_dismissable(severity) {
            return Err(AppError::validation(
                "FINDING_NOT_DISMISSABLE",
                "Errors and critical findings cannot be dismissed",
            ));
        }
        if fingerprint.trim().is_empty() {
            return Err(AppError::validation(
                "FINDING_INVALID",
                "A finding needs an identity to be dismissed",
            ));
        }
        let mut map = self.load()?;
        map.insert(fingerprint.to_string(), signature.to_string());
        self.save(&map)
    }

    pub fn restore(&self, fingerprint: &str) -> AppResult<()> {
        let mut map = self.load()?;
        if map.remove(fingerprint).is_some() {
            self.save(&map)?;
        }
        Ok(())
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

    fn service() -> FindingDismissals {
        FindingDismissals::new(Arc::new(Memory::default()))
    }

    #[test]
    fn a_dismissal_is_remembered_with_its_signature_and_can_be_restored() {
        let s = service();
        s.dismiss("fp1", "sig-a", "info").unwrap();
        s.dismiss("fp2", "sig-b", "Warning").unwrap();
        let listed = s.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].fingerprint, "fp1");
        assert_eq!(listed[0].signature, "sig-a");

        s.restore("fp1").unwrap();
        assert_eq!(s.list().unwrap().len(), 1);
        s.restore("missing").unwrap();
    }

    #[test]
    fn serious_findings_can_never_be_dismissed() {
        let s = service();
        for severity in ["critical", "error", "Error", "something-unknown"] {
            assert!(s.dismiss("fp", "sig", severity).is_err(), "{severity}");
        }
        assert!(s.list().unwrap().is_empty());
    }

    #[test]
    fn dismissing_again_replaces_the_signature() {
        let s = service();
        s.dismiss("fp", "old", "info").unwrap();
        s.dismiss("fp", "new", "info").unwrap();
        assert_eq!(s.list().unwrap()[0].signature, "new");
    }

    #[test]
    fn corrupt_stored_data_is_treated_as_no_dismissals() {
        let memory = Arc::new(Memory::default());
        memory.set_preference(KEY, "{not json").unwrap();
        let s = FindingDismissals::new(memory);
        assert!(s.list().unwrap().is_empty());
    }
}
