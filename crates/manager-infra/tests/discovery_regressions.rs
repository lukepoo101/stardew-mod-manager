#![cfg(unix)]

//! Discovery and inspection regressions.
//!
//! These run on every POSIX host: the layouts, the Steam VDF reader and the
//! inspector are data, so the same behaviour is asserted wherever they run.

use manager_app::ports::discovery::{GameCandidate, GameDiscoveryPort};
use manager_app::services::GamesService;
use manager_core::game::{ManagementMode, OperatingSystem, Storefront, SupportState};
use manager_infra::db::SqliteStateRepository;
use manager_infra::discovery::{PosixGameInspector, SteamGameDiscovery};
use manager_infra::platform::host_semantics::HostPathSemantics;
use std::{path::PathBuf, sync::Arc};

struct Discovery(Vec<PathBuf>);

impl GameDiscoveryPort for Discovery {
    fn operating_system(&self) -> OperatingSystem {
        OperatingSystem::host()
    }

    fn discover(&self) -> Vec<GameCandidate> {
        self.0
            .iter()
            .map(|path| GameCandidate {
                path: path.clone(),
                storefront: Storefront::Steam,
                operating_system: OperatingSystem::host(),
            })
            .collect()
    }
}

fn games_service(repo: Arc<SqliteStateRepository>, discovery: Arc<Discovery>) -> GamesService {
    GamesService::new(
        repo.clone(),
        repo.clone(),
        repo,
        discovery,
        Arc::new(PosixGameInspector::new()),
        Arc::new(HostPathSemantics::new()),
    )
}

/// A synthetic POSIX installation: the native launcher and the managed assembly.
fn write_posix_game(game: &std::path::Path) {
    std::fs::create_dir_all(game).unwrap();
    std::fs::write(game.join("StardewValley"), b"#!/bin/sh\n").unwrap();
    std::fs::write(game.join("Stardew Valley.dll"), b"fixture").unwrap();
}

#[test]
fn discovery_deduplicates_registered_paths_and_revalidates_missing_games() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    write_posix_game(&game);
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&game, &alias).unwrap();
    let repo = Arc::new(SqliteStateRepository::new_in_memory().unwrap());
    let service = games_service(repo, Arc::new(Discovery(vec![game.clone(), alias])));
    let accepted = service
        .accept_game(&game, Storefront::Steam, ManagementMode::Managed)
        .unwrap();
    let found = service.discover_games().unwrap();
    assert_eq!(found.len(), 1, "a symlinked alias is the same installation");
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
fn a_registered_installation_is_recognised_through_a_different_path_spelling() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("Stardew Valley");
    write_posix_game(&game);
    let repo = Arc::new(SqliteStateRepository::new_in_memory().unwrap());
    let service = games_service(repo, Arc::new(Discovery(Vec::new())));
    let accepted = service
        .accept_game(&game, Storefront::Steam, ManagementMode::Managed)
        .unwrap();

    // Registering the same directory again must reuse the identifier rather
    // than creating a second installation: identity is a comparison key, not a
    // path string.
    let again = service
        .accept_game(&game, Storefront::Steam, ManagementMode::Managed)
        .unwrap();
    assert_eq!(again.id, accepted.id);
    assert_eq!(service.list_games().unwrap().len(), 1);
}

#[test]
fn inspection_does_not_overwrite_existing_probe_files_or_invent_versions() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path();
    write_posix_game(game);
    std::fs::write(game.join(".smm_probe_write"), b"user data").unwrap();
    let inspection = PosixGameInspector::inspect_path(game, Storefront::Steam).unwrap();
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
        PosixGameInspector::inspect_path(game, Storefront::Steam)
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
    let declared = secondary.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        steam.join("steamapps/libraryfolders.vdf"),
        format!("\"path\" \"{}\"", declared),
    )
    .unwrap();
    std::fs::write(game.join("Stardew Valley.exe"), b"windows").unwrap();
    assert_eq!(
        SteamGameDiscovery::discover_installations_from_roots(&[steam]).len(),
        1
    );

    // A Windows-only installation is reported as another platform rather than
    // as an invalid directory, because that is what the user selected.
    assert_eq!(
        PosixGameInspector::inspect_path(&game, Storefront::Steam)
            .unwrap()
            .operating_system,
        OperatingSystem::Linux
    );
    assert_eq!(
        PosixGameInspector::inspect_path(&game, Storefront::Steam)
            .unwrap()
            .support_state,
        SupportState::InvalidGameDirectory
    );
}
