//! A profile that last loaded its mods with one game/SMAPI version is warned
//! about a different one, and an unreadable version never raises a warning.

use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::discovery::GameInstallationInspectorPort;
use manager_app::ports::repositories::{
    GameInstallationRepository, ProfileRepository, SmapiRepository,
};
use manager_app::services::{HealthService, RuntimeObserver};
use manager_core::game::{
    GameInspection, GameInstallation, ManagementMode, OperatingSystem, Storefront, SupportState,
};
use manager_core::ids::GameInstallationId;
use manager_core::launch::RuntimeVersions;
use manager_core::profile::Profile;
use manager_core::smapi::ManagedSmapiInstallation;
use manager_infra::db::SqliteStateRepository;
use std::path::Path;
use std::sync::{Arc, Mutex};

struct FakeInspector {
    game_version: Mutex<Option<String>>,
}

impl GameInstallationInspectorPort for FakeInspector {
    fn inspect(
        &self,
        path: &Path,
        storefront: Storefront,
        operating_system: OperatingSystem,
    ) -> AppResult<GameInspection> {
        Ok(GameInspection {
            installation_id: None,
            canonical_root: path.to_path_buf(),
            operating_system,
            storefront,
            observed_game_version: self.game_version.lock().unwrap().clone(),
            observed_smapi_version: None,
            has_existing_smapi: false,
            has_existing_mods: false,
            is_writable: true,
            support_state: SupportState::SupportedManaged,
            evidence: Vec::new(),
            inspected_at: Utc::now(),
        })
    }
}

struct Fixture {
    repo: Arc<SqliteStateRepository>,
    inspector: Arc<FakeInspector>,
    observer: Arc<RuntimeObserver>,
    health: HealthService,
    profile: Profile,
    game_id: GameInstallationId,
    _tmp: tempfile::TempDir,
}

fn fixture(game_version: Option<&str>) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("s.sqlite3")).unwrap());
    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();
    let profile = Profile::new(game.id, "Default");
    repo.save_profile(&profile).unwrap();
    repo.save_smapi_installation(&ManagedSmapiInstallation {
        game_installation_id: game.id,
        release_version: "4.1.10".to_string(),
        release_policy_id: "pinned".to_string(),
        installed_at: Utc::now(),
    })
    .unwrap();

    let inspector = Arc::new(FakeInspector {
        game_version: Mutex::new(game_version.map(str::to_string)),
    });
    let observer = Arc::new(RuntimeObserver::new(
        repo.clone(),
        repo.clone(),
        repo.clone(),
        inspector.clone(),
    ));
    let health = HealthService::new(
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
    )
    .with_runtime_observer(observer.clone());
    Fixture {
        repo,
        inspector,
        observer,
        health,
        profile,
        game_id: game.id,
        _tmp: tmp,
    }
}

fn runtime_codes(fixture: &Fixture) -> Vec<String> {
    fixture
        .health
        .get_health_summary(Some(&fixture.profile.id))
        .unwrap()
        .findings
        .into_iter()
        .filter(|f| f.category == "runtime" && f.code.starts_with("RUNTIME_"))
        .map(|f| f.code)
        .collect()
}

#[test]
fn nothing_is_reported_before_a_profile_has_a_working_record() {
    let f = fixture(Some("1.6.15"));
    assert!(runtime_codes(&f).is_empty());
}

#[test]
fn unchanged_versions_are_quiet_and_changed_ones_are_named() {
    let f = fixture(Some("1.6.15"));
    let versions = f.observer.observe(&f.game_id).unwrap();
    assert_eq!(versions.game_version.as_deref(), Some("1.6.15"));
    assert_eq!(versions.smapi_version.as_deref(), Some("4.1.10"));
    f.observer.remember(&f.profile.id, &versions).unwrap();
    assert!(runtime_codes(&f).is_empty());

    *f.inspector.game_version.lock().unwrap() = Some("1.6.16".to_string());
    assert_eq!(runtime_codes(&f), vec!["RUNTIME_GAME_CHANGED"]);

    f.repo
        .save_smapi_installation(&ManagedSmapiInstallation {
            game_installation_id: f.game_id,
            release_version: "4.2.0".to_string(),
            release_policy_id: "pinned".to_string(),
            installed_at: Utc::now(),
        })
        .unwrap();
    let mut codes = runtime_codes(&f);
    codes.sort();
    assert_eq!(codes, vec!["RUNTIME_GAME_CHANGED", "RUNTIME_SMAPI_CHANGED"]);
}

#[test]
fn an_unreadable_current_version_is_not_reported_as_a_change() {
    let f = fixture(Some("1.6.15"));
    let versions = f.observer.observe(&f.game_id).unwrap();
    f.observer.remember(&f.profile.id, &versions).unwrap();
    *f.inspector.game_version.lock().unwrap() = None;
    assert!(runtime_codes(&f).is_empty());
    let _ = RuntimeVersions::default();
}
