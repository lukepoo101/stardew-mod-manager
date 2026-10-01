//! A game can have at most one default profile. It is used only when no
//! profile is explicitly active, and never replaces an explicit choice.

use chrono::Utc;
use manager_app::ports::repositories::{GameInstallationRepository, ProfileRepository};
use manager_app::services::ProfilesService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::GameInstallationId;
use manager_core::profile::{Profile, ProfileState};
use manager_infra::db::SqliteStateRepository;
use std::sync::Arc;

struct Fixture {
    repo: Arc<SqliteStateRepository>,
    service: ProfilesService,
    game: GameInstallationId,
    _tmp: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("state.sqlite3")).unwrap());
    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();
    let service = ProfilesService::new(repo.clone(), repo.clone(), repo.clone());
    Fixture {
        repo,
        service,
        game: game.id,
        _tmp: tmp,
    }
}

impl Fixture {
    fn profile(&self, name: &str, state: ProfileState) -> Profile {
        let mut profile = Profile::new(self.game, name);
        profile.state = state;
        self.repo.save_profile(&profile).unwrap();
        profile
    }

    fn defaults(&self) -> Vec<String> {
        self.service
            .list_profiles(&self.game)
            .unwrap()
            .into_iter()
            .filter(|p| p.is_default)
            .map(|p| p.name)
            .collect()
    }

    fn active(&self) -> Option<String> {
        self.service
            .get_active_profile(&self.game)
            .unwrap()
            .map(|p| p.name)
    }
}

#[test]
fn setting_a_new_default_replaces_the_previous_one() {
    let f = fixture();
    let main = f.profile("Main", ProfileState::Active);
    let other = f.profile("Other", ProfileState::Active);
    assert!(f.defaults().is_empty());

    f.service
        .set_default_profile(&f.game, Some(&main.id))
        .unwrap();
    assert_eq!(f.defaults(), vec!["Main"]);
    f.service
        .set_default_profile(&f.game, Some(&other.id))
        .unwrap();
    assert_eq!(f.defaults(), vec!["Other"]);
    f.service.set_default_profile(&f.game, None).unwrap();
    assert!(f.defaults().is_empty());
}

#[test]
fn the_default_is_used_only_when_nothing_is_explicitly_active() {
    let f = fixture();
    let main = f.profile("Main", ProfileState::Active);
    let other = f.profile("Other", ProfileState::Active);
    assert_eq!(f.active(), None);

    f.service
        .set_default_profile(&f.game, Some(&main.id))
        .unwrap();
    assert_eq!(f.active().as_deref(), Some("Main"));

    f.service.switch_active_profile(&f.game, &other.id).unwrap();
    assert_eq!(f.active().as_deref(), Some("Other"));
    // Changing the default does not move the user off their chosen profile.
    f.service
        .set_default_profile(&f.game, Some(&main.id))
        .unwrap();
    assert_eq!(f.active().as_deref(), Some("Other"));
}

#[test]
fn only_playable_profiles_of_the_same_game_can_be_default() {
    let f = fixture();
    let archived = f.profile("Old", ProfileState::Archived);
    let error = f
        .service
        .set_default_profile(&f.game, Some(&archived.id))
        .unwrap_err();
    assert_eq!(error.code, "PROFILE_NOT_SELECTABLE");

    let elsewhere = Profile::new(GameInstallationId::new(), "Elsewhere");
    let other_game = GameInstallation {
        id: elsewhere.game_installation_id,
        canonical_root: std::env::temp_dir().join("OtherGame"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    f.repo.save_game(&other_game).unwrap();
    f.repo.save_profile(&elsewhere).unwrap();
    let error = f
        .service
        .set_default_profile(&f.game, Some(&elsewhere.id))
        .unwrap_err();
    assert_eq!(error.code, "PROFILE_GAME_MISMATCH");
    assert!(f.defaults().is_empty());
}
