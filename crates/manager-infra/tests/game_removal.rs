#![cfg(unix)]

//! Removing a registered installation that was never used: going Back in
//! setup, or tidying up Settings.

use chrono::Utc;
use manager_app::ports::discovery::{GameCandidate, GameDiscoveryPort};
use manager_app::ports::repositories::{
    GameInstallationRepository, OperationRepository, PreferencesRepository, ProfileRepository,
    SmapiRepository,
};
use manager_app::services::GamesService;
use manager_core::game::{ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::GameInstallationId;
use manager_core::profile::{Profile, ProfileState};
use manager_core::smapi::ManagedSmapiInstallation;
use manager_infra::db::SqliteStateRepository;
use manager_infra::discovery::PosixGameInspector;
use manager_infra::platform::host_semantics::HostPathSemantics;
use std::str::FromStr;
use std::sync::Arc;

struct NoDiscovery;

impl GameDiscoveryPort for NoDiscovery {
    fn operating_system(&self) -> OperatingSystem {
        OperatingSystem::host()
    }

    fn discover(&self) -> Vec<GameCandidate> {
        Vec::new()
    }
}

struct Fixture {
    repo: Arc<SqliteStateRepository>,
    service: GamesService,
    root: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let repo = Arc::new(SqliteStateRepository::new_in_memory().unwrap());
    let service = GamesService::new(
        repo.clone(),
        repo.clone(),
        repo.clone(),
        Arc::new(NoDiscovery),
        Arc::new(PosixGameInspector),
        Arc::new(HostPathSemantics::new()),
    );
    Fixture {
        repo,
        service,
        root: tempfile::tempdir().unwrap(),
    }
}

impl Fixture {
    /// Writes a synthetic game folder and registers it as setup does.
    fn register(&self, name: &str) -> GameInstallationId {
        let game = self.root.path().join(name);
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("StardewValley"), b"#!/bin/sh\n").unwrap();
        std::fs::write(game.join("Stardew Valley.dll"), b"fixture").unwrap();
        let dto = self
            .service
            .accept_game(&game, Storefront::Manual, ManagementMode::Managed)
            .unwrap();
        assert!(dto.unused, "a just-registered installation is unused");
        GameInstallationId::from_str(&dto.id).unwrap()
    }

    fn active(&self) -> Option<GameInstallationId> {
        self.repo
            .get_app_context()
            .unwrap()
            .active_game_installation_id
    }
}

#[test]
fn going_back_in_setup_forgets_the_new_installation_and_its_profile() {
    let f = fixture();
    let kept = f.register("Stardew Valley");
    let mistaken = f.register("emptymods");
    assert_eq!(f.active(), Some(mistaken));
    assert_eq!(f.repo.list_profiles(&mistaken).unwrap().len(), 1);

    f.service.remove_unused_game(&mistaken, Some(kept)).unwrap();

    let listed = f.service.list_games().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, kept.to_string());
    assert!(f.repo.list_profiles(&mistaken).unwrap().is_empty());
    assert_eq!(f.active(), Some(kept), "the previous game is active again");
    // Creating its "Main" profile is not left behind in Activity.
    assert!(f
        .repo
        .list_recent_operations(50)
        .unwrap()
        .iter()
        .all(|op| op.game_installation_id != Some(mistaken)));
}

#[test]
fn removing_the_only_installation_leaves_no_active_game() {
    let f = fixture();
    let only = f.register("Stardew Valley");
    f.service.remove_unused_game(&only, None).unwrap();
    assert!(f.service.list_games().unwrap().is_empty());
    assert_eq!(f.active(), None);
}

#[test]
fn an_installation_in_use_is_refused_and_left_as_it_was() {
    let f = fixture();
    let used = f.register("Stardew Valley");
    f.repo
        .save_smapi_installation(&ManagedSmapiInstallation {
            game_installation_id: used,
            release_version: "4.1.10".into(),
            release_policy_id: "test".into(),
            installed_at: Utc::now(),
        })
        .unwrap();
    assert!(!f.service.list_games().unwrap()[0].unused);

    let error = f.service.remove_unused_game(&used, None).unwrap_err();
    assert_eq!(error.code, "GAME_INSTALLATION_IN_USE");
    assert!(error.summary.contains("SMAPI"), "{}", error.summary);
    assert_eq!(f.service.list_games().unwrap().len(), 1);
    assert_eq!(f.repo.list_profiles(&used).unwrap().len(), 1);
    assert_eq!(f.active(), Some(used));
}

/// What the user made counts as use, even with no mods, launches or SMAPI.
fn assert_kept(f: &Fixture, game: GameInstallationId, reason: &str) {
    let listed = f.service.list_games().unwrap();
    let dto = listed.iter().find(|g| g.id == game.to_string()).unwrap();
    assert!(!dto.unused, "{reason} should count as use");
    let error = f.service.remove_unused_game(&game, None).unwrap_err();
    assert_eq!(error.code, "GAME_INSTALLATION_IN_USE", "{reason}");
    assert!(f.repo.get_game(&game).unwrap().is_some(), "{reason}");
}

#[test]
fn profiles_the_user_made_or_changed_count_as_use() {
    let f = fixture();
    let extra = f.register("extra");
    f.repo
        .save_profile(&Profile::new(extra, "Co-op with Sam"))
        .unwrap();
    assert_kept(&f, extra, "a second profile");
    assert_eq!(f.repo.list_profiles(&extra).unwrap().len(), 2);

    let renamed = f.register("renamed");
    let main = f.repo.list_profiles(&renamed).unwrap().remove(0);
    f.repo
        .update_profile_details(&main.id, "Old farm", None, Utc::now())
        .unwrap();
    assert_kept(&f, renamed, "a renamed default profile");

    let archived = f.register("archived");
    let mut main = f.repo.list_profiles(&archived).unwrap().remove(0);
    main.state = ProfileState::Archived;
    f.repo.save_profile(&main).unwrap();
    assert_kept(&f, archived, "an archived default profile");
}

#[test]
fn linked_saves_and_restore_points_count_as_use() {
    let f = fixture();
    let linked = f.register("linked");
    let main = f.repo.list_profiles(&linked).unwrap().remove(0);
    f.repo
        .set_preference(
            "save_associations",
            &format!(r#"{{"Farm_123":"{}"}}"#, main.id),
        )
        .unwrap();
    assert_kept(&f, linked, "a linked save");

    let pointed = f.register("pointed");
    let main = f.repo.list_profiles(&pointed).unwrap().remove(0);
    f.repo
        .set_preference(&format!("restore_points:{}", main.id), "[]")
        .unwrap();
    assert_kept(&f, pointed, "a restore point");
}

#[test]
fn an_installation_added_without_managing_it_is_never_unused() {
    let f = fixture();
    let game = f.root.path().join("modded");
    std::fs::create_dir_all(game.join("Mods/SomeMod")).unwrap();
    std::fs::write(game.join("StardewValley"), b"#!/bin/sh\n").unwrap();
    std::fs::write(game.join("Stardew Valley.dll"), b"fixture").unwrap();
    std::fs::write(
        game.join("Mods/SomeMod/manifest.json"),
        br#"{"UniqueID":"A.B","Name":"B","Author":"A","Version":"1.0.0"}"#,
    )
    .unwrap();
    let dto = f
        .service
        .accept_game(&game, Storefront::Manual, ManagementMode::ExternalUnmanaged)
        .unwrap();
    assert_kept(
        &f,
        GameInstallationId::from_str(&dto.id).unwrap(),
        "a deliberately unmanaged installation",
    );
}
