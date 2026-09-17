//! Shared Steam discovery orchestration.
//!
//! Discovering the Steam client installation is platform-specific: Linux uses
//! well-known directories under the user's home, Windows uses the registry and
//! the default install locations. Everything after that is shared, because
//! libraryfolders.vdf has the same shape everywhere: enumerate the configured
//! libraries, look for the game's install directory in each, validate it
//! against the app manifest when one is present, and deduplicate the results
//! under host path semantics.

use crate::platform::shared::vdf;
use manager_app::ports::discovery::GameCandidate;
use manager_core::game::{OperatingSystem, Storefront, GAME_APP_ID};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where a platform looks for the Steam client installation.
pub trait SteamRootLocator: Send + Sync {
    /// The operating system whose Steam installs this locator describes.
    fn operating_system(&self) -> OperatingSystem;

    /// Steam client roots to inspect, whether or not they exist.
    fn steam_roots(&self) -> Vec<PathBuf>;
}

/// The directory name Steam uses for the game inside a library.
pub const GAME_DIRECTORY_NAME: &str = "Stardew Valley";
/// The library subdirectory that holds installed applications.
pub const STEAMAPPS_DIRECTORY: &str = "steamapps";

/// Discovers Stardew Valley installations through a platform's Steam locator.
pub fn discover_with_locator(locator: &dyn SteamRootLocator) -> Vec<GameCandidate> {
    let operating_system = locator.operating_system();
    discover_installations_from_roots(&locator.steam_roots(), operating_system)
        .into_iter()
        .map(|path| GameCandidate {
            path,
            storefront: Storefront::Steam,
            operating_system,
        })
        .collect()
}

/// Canonical roots of the Stardew Valley installations reachable from the given
/// Steam roots, including the libraries those roots declare.
pub fn discover_installations_from_roots(
    roots: &[PathBuf],
    operating_system: OperatingSystem,
) -> Vec<PathBuf> {
    let semantics = manager_core::path_semantics::host_path_semantics();
    let mut library_keys = BTreeSet::new();
    let mut libraries: Vec<PathBuf> = Vec::new();

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let canonical = root.canonicalize().unwrap_or_else(|_| root.clone());
        push_library(&mut libraries, &mut library_keys, canonical.clone());

        let vdf_path = canonical
            .join(STEAMAPPS_DIRECTORY)
            .join("libraryfolders.vdf");
        let Ok(content) = std::fs::read_to_string(&vdf_path) else {
            continue;
        };
        for declared in vdf::library_paths_from_vdf(&content) {
            // Steam writes absolute library paths; a relative one is either
            // corrupt or deliberately hostile.
            if declared.is_absolute() {
                push_library(&mut libraries, &mut library_keys, declared);
            }
        }
    }

    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for library in libraries {
        let steamapps = library.join(STEAMAPPS_DIRECTORY);
        let candidate = steamapps.join("common").join(GAME_DIRECTORY_NAME);
        if !candidate.is_dir() || !manifest_confirms_game(&steamapps) {
            continue;
        }
        let canonical = candidate.canonicalize().unwrap_or(candidate);
        if seen.insert(semantics.comparison_key(&canonical)) {
            found.push(canonical);
        }
    }

    let _ = operating_system;
    found
}

fn push_library(libraries: &mut Vec<PathBuf>, keys: &mut BTreeSet<String>, candidate: PathBuf) {
    // Only absolute library roots are ever considered: Steam records absolute
    // paths, and a relative one would resolve against the manager's own
    // working directory.
    if !candidate.is_absolute() || !candidate.is_dir() {
        return;
    }
    let semantics = manager_core::path_semantics::host_path_semantics();
    let canonical = candidate.canonicalize().unwrap_or(candidate);
    if keys.insert(semantics.comparison_key(&canonical)) {
        libraries.push(canonical);
    }
}

/// Whether the library's app manifest records the Stardew Valley app id.
///
/// A library with no manifest is still accepted: a partially installed or
/// manually copied library is a real situation the user can explain, and the
/// inspector makes the final call on whether the directory is usable.
pub fn manifest_confirms_game(steamapps: &Path) -> bool {
    let manifest = steamapps.join(format!("appmanifest_{}.acf", GAME_APP_ID));
    let Ok(content) = std::fs::read_to_string(&manifest) else {
        return true;
    };
    match vdf::parse_vdf(&content)
        .get("AppState")
        .and_then(|state| state.get_scalar("appid"))
    {
        Some(app_id) => app_id.trim() == GAME_APP_ID,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedLocator {
        operating_system: OperatingSystem,
        roots: Vec<PathBuf>,
    }

    impl SteamRootLocator for FixedLocator {
        fn operating_system(&self) -> OperatingSystem {
            self.operating_system
        }

        fn steam_roots(&self) -> Vec<PathBuf> {
            self.roots.clone()
        }
    }

    /// A filesystem path as Steam would write it inside a VDF file: the
    /// separator is escaped, because a backslash is the VDF escape character.
    fn vdf_path(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "\\\\")
    }

    fn steam_library(root: &Path, game_files: &[&str]) {
        let game = root
            .join(STEAMAPPS_DIRECTORY)
            .join("common")
            .join(GAME_DIRECTORY_NAME);
        std::fs::create_dir_all(&game).unwrap();
        for file in game_files {
            std::fs::write(game.join(file), b"fixture").unwrap();
        }
    }

    #[test]
    fn a_library_declared_by_vdf_is_searched() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("Steam");
        let secondary = tmp.path().join("Secondary Library");
        std::fs::create_dir_all(primary.join(STEAMAPPS_DIRECTORY)).unwrap();
        std::fs::write(
            primary
                .join(STEAMAPPS_DIRECTORY)
                .join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
                vdf_path(&primary),
                vdf_path(&secondary)
            ),
        )
        .unwrap();
        steam_library(&secondary, &["Stardew Valley.exe"]);

        let locator = FixedLocator {
            operating_system: OperatingSystem::Linux,
            roots: vec![primary],
        };
        let candidates = discover_with_locator(&locator);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].storefront, Storefront::Steam);
        assert_eq!(
            candidates[0].operating_system,
            OperatingSystem::Linux,
            "the locator's platform is what the candidate reports"
        );
    }

    #[test]
    fn an_empty_library_never_produces_a_candidate() {
        let tmp = tempfile::tempdir().unwrap();
        let steam = tmp.path().join("Steam");
        std::fs::create_dir_all(steam.join(STEAMAPPS_DIRECTORY)).unwrap();
        let locator = FixedLocator {
            operating_system: OperatingSystem::Windows,
            roots: vec![steam],
        };
        assert!(discover_with_locator(&locator).is_empty());
    }

    #[test]
    fn a_manifest_for_another_app_rejects_the_library() {
        let tmp = tempfile::tempdir().unwrap();
        let steam = tmp.path().join("Steam");
        steam_library(&steam, &["Stardew Valley.exe"]);
        std::fs::write(
            steam
                .join(STEAMAPPS_DIRECTORY)
                .join(format!("appmanifest_{}.acf", GAME_APP_ID)),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"620\"\n}\n",
        )
        .unwrap();
        assert!(!manifest_confirms_game(&steam.join(STEAMAPPS_DIRECTORY)));
    }

    #[test]
    fn a_missing_manifest_still_allows_the_library() {
        let tmp = tempfile::tempdir().unwrap();
        let steam = tmp.path().join("Steam");
        std::fs::create_dir_all(steam.join(STEAMAPPS_DIRECTORY)).unwrap();
        assert!(manifest_confirms_game(&steam.join(STEAMAPPS_DIRECTORY)));
    }

    #[test]
    fn malformed_library_folders_does_not_hide_the_primary_library() {
        let tmp = tempfile::tempdir().unwrap();
        let steam = tmp.path().join("Steam");
        steam_library(&steam, &["Stardew Valley.exe"]);
        std::fs::write(
            steam.join(STEAMAPPS_DIRECTORY).join("libraryfolders.vdf"),
            "this is not a vdf file {{{",
        )
        .unwrap();

        let locator = FixedLocator {
            operating_system: OperatingSystem::Linux,
            roots: vec![steam],
        };
        assert_eq!(discover_with_locator(&locator).len(), 1);
    }

    #[test]
    fn a_bare_path_declaration_naming_an_existing_posix_library_is_searched() {
        // Steam's legacy file puts the path directly against a key, and the
        // discovery has to accept that shape as well as the nested one.
        let tmp = tempfile::tempdir().unwrap();
        let steam = tmp.path().join("Steam");
        let secondary = tmp.path().join("Secondary Library");
        let game = secondary
            .join(STEAMAPPS_DIRECTORY)
            .join("common")
            .join(GAME_DIRECTORY_NAME);
        std::fs::create_dir_all(steam.join(STEAMAPPS_DIRECTORY)).unwrap();
        std::fs::create_dir_all(&game).unwrap();

        let declared = secondary.to_string_lossy().replace('\\', "\\\\");
        let content = format!("\"path\" \"{}\"", declared);
        let parsed = vdf::library_paths_from_vdf(&content);
        assert_eq!(parsed.len(), 1, "parsed: {parsed:?} from {content:?}");

        std::fs::write(
            steam.join(STEAMAPPS_DIRECTORY).join("libraryfolders.vdf"),
            &content,
        )
        .unwrap();
        let roots = discover_installations_from_roots(&[steam], OperatingSystem::Linux);
        assert_eq!(roots.len(), 1, "roots: {roots:?} from {content:?}");
    }

    #[test]
    fn a_declared_secondary_library_is_found_in_the_posix_shape() {
        // This mirrors the POSIX discovery regression exactly: the declared
        // library holds the game, and the primary library holds only the VDF.
        let tmp = tempfile::tempdir().unwrap();
        let steam = tmp.path().join("Steam");
        let secondary = tmp.path().join("Secondary Library");
        let game = secondary
            .join(STEAMAPPS_DIRECTORY)
            .join("common")
            .join(GAME_DIRECTORY_NAME);
        std::fs::create_dir_all(steam.join(STEAMAPPS_DIRECTORY)).unwrap();
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(
            steam.join(STEAMAPPS_DIRECTORY).join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
                vdf_path(&secondary)
            ),
        )
        .unwrap();
        let roots = discover_installations_from_roots(&[steam], OperatingSystem::Linux);
        assert_eq!(roots.len(), 1, "declared library was not searched");
    }

    #[test]
    fn a_relative_library_path_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let steam = tmp.path().join("Steam");
        std::fs::create_dir_all(steam.join(STEAMAPPS_DIRECTORY)).unwrap();
        std::fs::write(
            steam.join(STEAMAPPS_DIRECTORY).join("libraryfolders.vdf"),
            "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"../escape\"\n\t}\n}\n",
        )
        .unwrap();
        let roots = discover_installations_from_roots(&[steam], OperatingSystem::Linux);
        assert!(roots.is_empty());
    }
}
