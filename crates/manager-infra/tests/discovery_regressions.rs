use manager_app::ports::discovery::GameDiscoveryPort;
use manager_app::services::GamesService;
use manager_core::game::{ManagementMode, Storefront, SupportState};
use manager_infra::db::SqliteStateRepository;
use manager_infra::discovery::{LinuxGameInspector, SteamGameDiscovery};
use std::{path::PathBuf, sync::Arc};

struct Discovery(Vec<PathBuf>);
impl GameDiscoveryPort for Discovery {
    fn discover(&self) -> Vec<(PathBuf, Storefront)> {
        self.0
            .iter()
            .map(|path| (path.clone(), Storefront::Steam))
            .collect()
    }
}

#[test]
fn discovery_deduplicates_registered_paths_and_revalidates_missing_games() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    std::fs::create_dir(&game).unwrap();
    std::fs::write(game.join("StardewValley"), b"#!/bin/sh\n").unwrap();
    std::fs::write(game.join("Stardew Valley.dll"), b"fixture").unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&game, &alias).unwrap();
    let repo = Arc::new(SqliteStateRepository::new_in_memory().unwrap());
    let service = GamesService::new(
        repo.clone(),
        repo.clone(),
        repo,
        Arc::new(Discovery(vec![game.clone(), alias])),
        Arc::new(LinuxGameInspector::new()),
    );
    let accepted = service
        .accept_game(&game, Storefront::Steam, ManagementMode::Managed)
        .unwrap();
    let found = service.discover_games().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].support_state, "supported_managed");
    assert_eq!(
        service
            .accept_game(&game, Storefront::Steam, ManagementMode::Managed)
            .unwrap()
            .id,
        accepted.id
    );
    std::fs::remove_file(game.join("StardewValley")).unwrap();
    assert!(!service.inspect_path(&game, None).unwrap().is_usable);
    assert!(!service.discover_games().unwrap()[0].is_usable);
    assert!(service
        .accept_game(&game, Storefront::Steam, ManagementMode::Managed)
        .is_err());
}

#[test]
fn inspection_does_not_overwrite_existing_probe_files_or_invent_versions() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path();
    std::fs::write(game.join("StardewValley"), b"#!/bin/sh\n").unwrap();
    std::fs::write(game.join("Stardew Valley.dll"), b"fixture").unwrap();
    std::fs::write(game.join(".smm_probe_write"), b"user data").unwrap();
    let inspection = LinuxGameInspector::inspect_path(game, Storefront::Steam).unwrap();
    assert_eq!(inspection.support_state, SupportState::SupportedFresh);
    assert_eq!(inspection.observed_game_version, None);
    assert_eq!(
        std::fs::read(game.join(".smm_probe_write")).unwrap(),
        b"user data"
    );
    std::fs::write(
        game.join("Stardew Valley.deps.json"),
        r#"{"targets":{"net6":{"Stardew Valley/1.6.15.24356":{}}}}"#,
    )
    .unwrap();
    assert_eq!(
        LinuxGameInspector::inspect_path(game, Storefront::Steam)
            .unwrap()
            .observed_game_version
            .as_deref(),
        Some("1.6.15.24356")
    );
}

#[test]
fn secondary_steam_libraries_are_found_and_windows_only_games_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let steam = root.path().join("Steam");
    let secondary = root.path().join("Secondary Library");
    let game = secondary.join("steamapps/common/Stardew Valley");
    std::fs::create_dir_all(steam.join("steamapps")).unwrap();
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(
        steam.join("steamapps/libraryfolders.vdf"),
        format!("\"path\" \"{}\"", secondary.display()),
    )
    .unwrap();
    std::fs::write(game.join("Stardew Valley.exe"), b"windows").unwrap();
    assert_eq!(
        SteamGameDiscovery::discover_installations_from_roots(&[steam]).len(),
        1
    );
    assert_eq!(
        LinuxGameInspector::inspect_path(&game, Storefront::Steam)
            .unwrap()
            .support_state,
        SupportState::UnsupportedPlatform
    );
}
