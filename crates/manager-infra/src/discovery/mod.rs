use manager_core::domain::{GameInstallation, StoreKind};
use manager_core::game::create_game_installation;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Default, Clone)]
pub struct SteamGameDiscovery;

impl SteamGameDiscovery {
    pub fn new() -> Self {
        Self
    }

    pub fn discover_installations() -> Vec<GameInstallation> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let home_path = Path::new(&home);

        let standard_steam_roots = [
            home_path.join(".local/share/Steam"),
            home_path.join(".steam/steam"),
            home_path.join(".steam/root"),
            home_path.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            home_path.join(".var/app/com.valvesoftware.Steam/data/Steam"),
        ];

        Self::discover_installations_from_roots(&standard_steam_roots)
    }

    pub fn discover_installations_from_roots(roots: &[PathBuf]) -> Vec<GameInstallation> {
        let mut candidates = Vec::new();
        let mut library_dirs = Vec::new();

        for root in roots {
            if root.exists() {
                let canonical_root = root.canonicalize().unwrap_or_else(|_| root.clone());
                if !library_dirs.contains(&canonical_root) {
                    library_dirs.push(canonical_root.clone());
                }
                let vdf_path = canonical_root.join("steamapps/libraryfolders.vdf");
                if vdf_path.exists() {
                    if let Ok(content) = fs::read_to_string(&vdf_path) {
                        for path in parse_vdf_library_paths(&content) {
                            let canonical_lib =
                                path.canonicalize().unwrap_or_else(|_| path.clone());
                            if !library_dirs.contains(&canonical_lib) {
                                library_dirs.push(canonical_lib);
                            }
                        }
                    }
                }
            }
        }

        let mut seen_canonicals = Vec::new();
        for lib in library_dirs {
            let candidate = lib.join("steamapps/common/Stardew Valley");
            if candidate.exists() {
                if let Ok(canonical) = candidate.canonicalize() {
                    if !seen_canonicals.contains(&canonical) {
                        seen_canonicals.push(canonical);
                    }
                }
            }
        }

        for (idx, canonical) in seen_canonicals.into_iter().enumerate() {
            let id = format!("steam-candidate-{}", idx + 1);
            let game = create_game_installation(&id, canonical, StoreKind::SteamNative);
            candidates.push(game);
        }

        candidates
    }
}

impl manager_app::ports::discovery::GameDiscoveryPort for SteamGameDiscovery {
    fn discover(&self) -> Vec<(PathBuf, manager_core::game::Storefront)> {
        let installations = Self::discover_installations();
        installations
            .into_iter()
            .map(|inst| (inst.canonical_root, manager_core::game::Storefront::Steam))
            .collect()
    }
}

pub struct LinuxGameInspector;

impl LinuxGameInspector {
    pub fn new() -> Self {
        Self
    }

    #[allow(clippy::result_large_err)]
    pub fn inspect_path(
        path: &Path,
        storefront: manager_core::game::Storefront,
    ) -> manager_app::error::AppResult<manager_core::game::GameInspection> {
        use chrono::Utc;
        use manager_core::game::{GameInspection, OperatingSystem, SupportState};
        let canonical_root = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut evidence = Vec::new();

        if !canonical_root.exists() || !canonical_root.is_dir() {
            return Ok(GameInspection {
                installation_id: None,
                canonical_root,
                operating_system: OperatingSystem::Linux,
                storefront,
                observed_game_version: None,
                observed_smapi_version: None,
                has_existing_smapi: false,
                has_existing_mods: false,
                is_writable: false,
                support_state: SupportState::InvalidGameDirectory,
                evidence: vec!["Directory does not exist".to_string()],
                inspected_at: Utc::now(),
            });
        }

        let has_native = [
            "Stardew Valley",
            "StardewValley",
            "StardewValley.bin.x86_64",
        ]
        .iter()
        .any(|name| canonical_root.join(name).is_file());
        let has_dll = canonical_root.join("Stardew Valley.dll").is_file();
        let has_windows = canonical_root.join("Stardew Valley.exe").is_file();
        let has_exe = (has_native && has_dll) || has_windows;
        if !has_exe {
            evidence.push(
                "Expected the native game launcher and Stardew Valley.dll in this folder".into(),
            );
        }
        if has_windows && !has_native {
            evidence.push("Windows game installations are not supported on Linux".into());
        }
        // A unique temporary file never truncates a user's existing file or follows a probe symlink.
        let is_writable = tempfile::Builder::new()
            .prefix(".smm-probe-")
            .tempfile_in(&canonical_root)
            .is_ok();
        if is_writable {
            evidence.push("Game directory is writable".to_string());
        } else {
            evidence.push("Game directory is NOT writable".to_string());
        }

        let has_smapi_bin = canonical_root.join("StardewModdingAPI").exists()
            || canonical_root.join("StardewModdingAPI.bin.x86_64").exists()
            || canonical_root.join("StardewModdingAPI.exe").exists()
            || canonical_root.join("smapi-internal").exists();

        if has_smapi_bin {
            evidence.push("Existing SMAPI installation detected".to_string());
        }

        let mods_dir = canonical_root.join("Mods");
        let mut has_mods = false;
        if mods_dir.exists() && mods_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&mods_dir) {
                for entry in entries.flatten() {
                    if let Ok(file_type) = entry.file_type() {
                        if file_type.is_dir() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name != "ConsoleCommands" && name != "SaveBackup" {
                                has_mods = true;
                                evidence.push(format!("Found existing mod directory: {}", name));
                                break;
                            }
                        }
                    }
                }
            }
        }

        let observed_game_version =
            fs::read_to_string(canonical_root.join("Stardew Valley.deps.json"))
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|deps| {
                    deps.get("targets")
                        .and_then(|value| value.as_object())
                        .and_then(|targets| {
                            targets
                                .values()
                                .filter_map(|target| target.as_object())
                                .flat_map(|target| target.keys())
                                .find_map(|key| {
                                    key.strip_prefix("Stardew Valley/").map(str::to_owned)
                                })
                        })
                });
        let observed_smapi_version =
            crate::smapi_adapter::detect_installed_smapi_version(&canonical_root);

        let support_state = manager_core::game::classify_game_support(
            has_exe,
            has_native,
            is_writable,
            has_smapi_bin,
            has_mods,
            false,
        );

        Ok(GameInspection {
            installation_id: None,
            canonical_root,
            operating_system: OperatingSystem::Linux,
            storefront,
            observed_game_version,
            observed_smapi_version,
            has_existing_smapi: has_smapi_bin,
            has_existing_mods: has_mods,
            is_writable,
            support_state,
            evidence,
            inspected_at: Utc::now(),
        })
    }
}

impl Default for LinuxGameInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl manager_app::ports::discovery::GameInstallationInspectorPort for LinuxGameInspector {
    fn inspect(
        &self,
        path: &Path,
        storefront: manager_core::game::Storefront,
    ) -> manager_app::error::AppResult<manager_core::game::GameInspection> {
        Self::inspect_path(path, storefront)
    }
}

pub fn parse_vdf_library_paths(vdf_content: &str) -> Vec<PathBuf> {
    parse_vdf_library_paths_with_filter(vdf_content, |p| p.exists())
}

pub fn parse_vdf_library_paths_with_filter<F: Fn(&Path) -> bool>(
    vdf_content: &str,
    exists: F,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for line in vdf_content.lines() {
        // Strip single-line comments
        let line_without_comment = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        let trimmed = line_without_comment.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Collect quoted tokens
        let mut tokens = Vec::new();
        let mut in_quote = false;
        let mut current_token = String::new();

        for ch in trimmed.chars() {
            if ch == '"' {
                if in_quote {
                    tokens.push(current_token.clone());
                    current_token.clear();
                    in_quote = false;
                } else {
                    in_quote = true;
                }
            } else if in_quote {
                current_token.push(ch);
            }
        }

        // Check modern format: "path" "/path/to/lib"
        if tokens.len() >= 2 {
            let key = &tokens[0];
            let val = &tokens[1];

            let is_path_key = key.eq_ignore_ascii_case("path")
                || key.starts_with("BaseInstallFolder_")
                || key.chars().all(|c| c.is_ascii_digit());

            if is_path_key && !val.is_empty() {
                let p = PathBuf::from(val);
                if exists(&p) && !paths.contains(&p) {
                    paths.push(p);
                }
            }
        }
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vdf_library_paths() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/tmp"
		"label"		""
	}
}
"#;
        let paths = parse_vdf_library_paths(vdf);
        assert!(!paths.is_empty());
        assert_eq!(paths[0], PathBuf::from("/tmp"));
    }

    #[test]
    fn test_parse_vdf_modern_and_secondary_drives() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/user/.local/share/Steam"
		"label"		""
	}
	"1"
	{
		"path"		"/mnt/games/SteamLibrary"
		"label"		"SSD Games"
	}
}
"#;
        let paths = parse_vdf_library_paths_with_filter(vdf, |_| true);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/home/user/.local/share/Steam"));
        assert_eq!(paths[1], PathBuf::from("/mnt/games/SteamLibrary"));
    }

    #[test]
    fn test_parse_vdf_legacy_steam_format() {
        let vdf = r#"
"LibraryFolders"
{
	"TimeNextStatsReport"		"123456789"
	"ContentStatsID"		"987654321"
	"1"		"/media/secondary/Steam"
	"BaseInstallFolder_2"		"/mnt/storage/SteamLibrary"
}
"#;
        let paths = parse_vdf_library_paths_with_filter(vdf, |_| true);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/media/secondary/Steam"));
        assert_eq!(paths[1], PathBuf::from("/mnt/storage/SteamLibrary"));
    }

    #[test]
    fn test_parse_vdf_malformed_and_comments() {
        let vdf = r#"
// This is a comment at the top
"libraryfolders"
{
	// Another comment
	"0"
	{
		"path"		"/valid/path" // inline comment
	}
	"unclosed_string
	random binary garbage \x00\xFF
	"path"
}
"#;
        let paths = parse_vdf_library_paths_with_filter(vdf, |_| true);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/valid/path"));
    }

    #[test]
    fn test_parse_vdf_empty() {
        let paths = parse_vdf_library_paths_with_filter("", |_| true);
        assert!(paths.is_empty());
    }

    #[test]
    fn test_deduplicate_symlinked_steam_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let steam_dir = tmp.path().join("Steam");
        let game_dir = steam_dir.join("steamapps/common/Stardew Valley");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(game_dir.join("Stardew Valley.dll"), b"fake dll").unwrap();
        fs::write(game_dir.join("StardewValley"), b"\x7fELFfake").unwrap();

        // Create symlink to steam_dir
        let symlink_dir = tmp.path().join("SteamSymlink");
        std::os::unix::fs::symlink(&steam_dir, &symlink_dir).unwrap();

        let roots = vec![steam_dir, symlink_dir];
        let candidates = SteamGameDiscovery::discover_installations_from_roots(&roots);
        assert_eq!(
            candidates.len(),
            1,
            "Expected exactly 1 candidate despite symlinked Steam roots"
        );
    }

    #[test]
    fn test_linux_game_inspector_support_states() {
        use manager_app::ports::discovery::GameInstallationInspectorPort;
        use manager_core::game::{Storefront, SupportState};

        let inspector = LinuxGameInspector::new();
        let tmp = tempfile::tempdir().unwrap();
        let game_dir = tmp.path().join("game");

        // 1. Missing directory
        let inspection = inspector.inspect(&game_dir, Storefront::Steam).unwrap();
        assert_eq!(inspection.support_state, SupportState::InvalidGameDirectory);

        // 2. Valid fresh game
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(game_dir.join("Stardew Valley.dll"), b"fake dll").unwrap();
        fs::write(game_dir.join("Stardew Valley"), b"\x7fELFfake").unwrap();

        let inspection = inspector.inspect(&game_dir, Storefront::Steam).unwrap();
        assert_eq!(inspection.support_state, SupportState::SupportedFresh);
        assert!(inspection.is_writable);

        // 3. Existing modded unmanaged
        let mods_dir = game_dir.join("Mods").join("TestMod");
        fs::create_dir_all(&mods_dir).unwrap();

        let inspection = inspector.inspect(&game_dir, Storefront::Steam).unwrap();
        assert_eq!(
            inspection.support_state,
            SupportState::ExistingModdedUnmanaged
        );
        assert!(inspection.has_existing_mods);
    }
}
