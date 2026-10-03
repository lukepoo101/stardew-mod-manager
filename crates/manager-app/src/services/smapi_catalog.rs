//! The list of SMAPI releases the manager can offer, read from SMAPI's own
//! published sources and cached locally.
//!
//! The list is refreshed at most once a day unless asked. A game range,
//! once read for a release tag, never changes, so it is kept for good. When
//! nothing can be read online, the last list is used, and failing that the
//! one release this manager ships with.

use crate::error::{AppError, AppResult};
use crate::ports::repositories::PreferencesRepository;
use crate::ports::smapi_releases::SmapiReleaseSourcePort;
use chrono::{DateTime, Duration, Utc};
use manager_core::smapi::catalog::{builtin_release, sort_releases, SmapiRelease};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

const LIST_KEY: &str = "smapi_catalog";
const RANGES_KEY: &str = "smapi_game_ranges";
const INSTALLERS_KEY: &str = "smapi_installers";
const UPDATES_KEY: &str = "smapi_update_settings";
/// How often the list is read online without being asked.
const REFRESH_AFTER_HOURS: i64 = 24;
/// Game ranges are read for at most this many of the newest releases per
/// refresh; older ones are read when they are first needed.
const RANGES_PER_REFRESH: usize = 40;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredList {
    checked_at: String,
    releases: Vec<SmapiRelease>,
}

type Ranges = BTreeMap<String, (Option<String>, Option<String>)>;

/// The list as it stands, with where it came from.
#[derive(Debug, Clone)]
pub struct CatalogView {
    pub releases: Vec<SmapiRelease>,
    /// "online" (just read), "cached" (read earlier) or "builtin" (only the
    /// release this manager ships with).
    pub source: String,
    pub checked_at: Option<String>,
    /// Why reading online failed, when it did.
    pub error: Option<String>,
}

/// An installer this manager downloaded and verified before, kept so a
/// version can be reinstalled or rolled back to without the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptInstaller {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub installed_at: String,
}

pub struct SmapiCatalog {
    preferences: Arc<dyn PreferencesRepository>,
    source: Option<Arc<dyn SmapiReleaseSourcePort>>,
}

impl SmapiCatalog {
    pub fn new(
        preferences: Arc<dyn PreferencesRepository>,
        source: Option<Arc<dyn SmapiReleaseSourcePort>>,
    ) -> Self {
        Self {
            preferences,
            source,
        }
    }

    fn read<T: for<'de> Deserialize<'de> + Default>(&self, key: &str) -> T {
        self.preferences
            .get_preference(key)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    fn write<T: Serialize>(&self, key: &str, value: &T) -> AppResult<()> {
        let json = serde_json::to_string(value)
            .map_err(|e| AppError::internal("Could not save the SMAPI list", e.to_string()))?;
        self.preferences.set_preference(key, &json)
    }

    /// The list with every known game range filled in, the built-in release
    /// always included, newest first.
    fn assemble(&self, mut releases: Vec<SmapiRelease>) -> Vec<SmapiRelease> {
        let ranges: Ranges = self.read(RANGES_KEY);
        let builtin = builtin_release();
        if !releases.iter().any(|r| r.version == builtin.version) {
            releases.push(builtin.clone());
        }
        for release in &mut releases {
            if release.min_game.is_none() {
                if let Some((min, max)) = ranges.get(&release.tag) {
                    release.min_game = min.clone();
                    release.max_game = max.clone();
                } else if release.version == builtin.version {
                    release.min_game = builtin.min_game.clone();
                    release.max_game = builtin.max_game.clone();
                }
            }
            if release.sha256.is_none() && release.version == builtin.version {
                release.sha256 = builtin.sha256.clone();
            }
        }
        sort_releases(&mut releases);
        releases
    }

    /// What is known without going online.
    pub fn cached(&self) -> CatalogView {
        let stored: StoredList = self.read(LIST_KEY);
        let source = if stored.releases.is_empty() {
            "builtin"
        } else {
            "cached"
        };
        CatalogView {
            releases: self.assemble(stored.releases),
            source: source.to_string(),
            checked_at: (!stored.checked_at.is_empty()).then_some(stored.checked_at),
            error: None,
        }
    }

    fn fresh_enough(checked_at: &str) -> bool {
        DateTime::parse_from_rfc3339(checked_at)
            .map(|at| Utc::now() - at.with_timezone(&Utc) < Duration::hours(REFRESH_AFTER_HOURS))
            .unwrap_or(false)
    }

    /// Reads the list online when it is older than a day, or when `force`.
    /// A failure leaves the last list in place and says why.
    pub async fn refresh(&self, force: bool) -> CatalogView {
        let stored: StoredList = self.read(LIST_KEY);
        let Some(source) = &self.source else {
            return self.cached();
        };
        if !force && Self::fresh_enough(&stored.checked_at) {
            return self.cached();
        }
        let releases = match source.list_releases().await {
            Ok(releases) => releases,
            Err(error) => {
                let mut view = self.cached();
                view.error = Some(error.summary);
                return view;
            }
        };
        // Game ranges for the newest releases not read before.
        let mut ranges: Ranges = self.read(RANGES_KEY);
        let mut newest = releases.clone();
        sort_releases(&mut newest);
        for release in newest.iter().take(RANGES_PER_REFRESH) {
            if ranges.contains_key(&release.tag) {
                continue;
            }
            if let Ok(range) = source.game_range(&release.tag).await {
                ranges.insert(release.tag.clone(), range);
            }
        }
        let _ = self.write(RANGES_KEY, &ranges);
        let checked_at = Utc::now().to_rfc3339();
        let _ = self.write(
            LIST_KEY,
            &StoredList {
                checked_at: checked_at.clone(),
                releases: releases.clone(),
            },
        );
        CatalogView {
            releases: self.assemble(releases),
            source: "online".to_string(),
            checked_at: Some(checked_at),
            error: None,
        }
    }

    /// One release by version, from the cached list, or from an installer
    /// kept from an earlier install.
    pub fn find(&self, version: &str) -> Option<SmapiRelease> {
        if let Some(mut found) = self
            .cached()
            .releases
            .into_iter()
            .find(|r| r.version == version)
        {
            // A release without a published checksum uses the one recorded
            // when this manager first downloaded and installed it.
            if found.sha256.is_none() {
                found.sha256 = self
                    .kept_installers()
                    .into_iter()
                    .find(|k| k.version == version)
                    .map(|k| k.sha256);
            }
            return Some(found);
        }
        self.kept_installers()
            .into_iter()
            .find(|k| k.version == version)
            .map(|k| SmapiRelease {
                version: k.version.clone(),
                tag: k.version,
                published_at: None,
                prerelease: false,
                installer_url: k.url,
                sha256: Some(k.sha256),
                min_game: None,
                max_game: None,
                notes_url: String::new(),
            })
    }

    pub fn update_settings(&self) -> crate::api::dto::SmapiUpdateSettingsDto {
        self.read(UPDATES_KEY)
    }

    pub fn set_update_settings(
        &self,
        settings: &crate::api::dto::SmapiUpdateSettingsDto,
    ) -> AppResult<()> {
        if !matches!(settings.mode.as_str(), "notify" | "auto" | "off") {
            return Err(AppError::validation(
                "SMAPI_UPDATE_MODE",
                "Choose to be told, to update automatically, or neither",
            ));
        }
        self.write(UPDATES_KEY, settings)
    }

    /// Installers downloaded and verified before, newest install first.
    pub fn kept_installers(&self) -> Vec<KeptInstaller> {
        let mut kept: Vec<KeptInstaller> = self.read(INSTALLERS_KEY);
        kept.sort_by(|a, b| b.installed_at.cmp(&a.installed_at));
        kept
    }

    /// Records an installed version's verified installer; keeps the newest
    /// `keep` and returns the versions no longer kept.
    pub fn remember_installer(&self, installer: KeptInstaller, keep: usize) -> Vec<String> {
        let mut kept = self.kept_installers();
        kept.retain(|k| k.version != installer.version);
        kept.insert(0, installer);
        let dropped = kept.split_off(keep.min(kept.len()));
        let _ = self.write(INSTALLERS_KEY, &kept);
        dropped.into_iter().map(|k| k.version).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppResult;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Prefs(Mutex<BTreeMap<String, String>>);
    impl PreferencesRepository for Prefs {
        fn get_window_geometry(
            &self,
        ) -> AppResult<Option<crate::ports::repositories::WindowGeometryDto>> {
            Ok(None)
        }
        fn save_window_geometry(
            &self,
            _: &crate::ports::repositories::WindowGeometryDto,
        ) -> AppResult<()> {
            Ok(())
        }
        fn get_preference(&self, key: &str) -> AppResult<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set_preference(&self, key: &str, value: &str) -> AppResult<()> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
    }

    struct Source {
        fail: bool,
        range_calls: Mutex<usize>,
    }
    #[async_trait::async_trait]
    impl SmapiReleaseSourcePort for Source {
        async fn list_releases(&self) -> AppResult<Vec<SmapiRelease>> {
            if self.fail {
                return Err(AppError::network("OFFLINE", "no network"));
            }
            Ok(vec![SmapiRelease {
                version: "4.5.2".into(),
                tag: "4.5.2".into(),
                published_at: None,
                prerelease: false,
                installer_url: "https://example/4.5.2.zip".into(),
                sha256: Some("aa".into()),
                min_game: None,
                max_game: None,
                notes_url: String::new(),
            }])
        }
        async fn game_range(&self, _tag: &str) -> AppResult<(Option<String>, Option<String>)> {
            *self.range_calls.lock().unwrap() += 1;
            Ok((Some("1.6.14".into()), None))
        }
    }

    #[tokio::test]
    async fn reads_online_once_a_day_and_keeps_ranges_for_good() {
        let prefs = Arc::new(Prefs::default());
        let source = Arc::new(Source {
            fail: false,
            range_calls: Mutex::new(0),
        });
        let catalog = SmapiCatalog::new(prefs.clone(), Some(source.clone()));
        let first = catalog.refresh(false).await;
        assert_eq!(first.source, "online");
        let newest = &first.releases[0];
        assert_eq!(newest.version, "4.5.2");
        assert_eq!(newest.min_game.as_deref(), Some("1.6.14"));
        // The shipped release is always listed.
        assert!(first.releases.iter().any(|r| r.version == "4.1.10"));
        // Within a day nothing is read again; forced, the ranges are not.
        assert_eq!(catalog.refresh(false).await.source, "cached");
        catalog.refresh(true).await;
        assert_eq!(*source.range_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn offline_falls_back_to_the_last_list_then_the_shipped_release() {
        let prefs = Arc::new(Prefs::default());
        let offline = SmapiCatalog::new(
            prefs.clone(),
            Some(Arc::new(Source {
                fail: true,
                range_calls: Mutex::new(0),
            })),
        );
        let view = offline.refresh(true).await;
        assert_eq!(view.source, "builtin");
        assert_eq!(view.error.as_deref(), Some("no network"));
        assert_eq!(view.releases.len(), 1);
        assert_eq!(view.releases[0].min_game.as_deref(), Some("1.6.14"));
    }

    #[test]
    fn kept_installers_are_bounded_and_findable() {
        let catalog = SmapiCatalog::new(Arc::new(Prefs::default()), None);
        for (i, v) in ["4.3.0", "4.4.0", "4.5.0", "4.5.2"].iter().enumerate() {
            let dropped = catalog.remember_installer(
                KeptInstaller {
                    version: v.to_string(),
                    url: format!("https://example/{v}"),
                    sha256: "ff".into(),
                    installed_at: format!("2026-10-0{}T00:00:00Z", i + 1),
                },
                3,
            );
            if *v == "4.5.2" {
                assert_eq!(dropped, vec!["4.3.0".to_string()]);
            }
        }
        assert_eq!(catalog.kept_installers().len(), 3);
        assert_eq!(catalog.find("4.4.0").unwrap().sha256.as_deref(), Some("ff"));
    }
}
