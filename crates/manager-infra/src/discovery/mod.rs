use manager_core::domain::{GameInstallation, StoreKind};
use manager_core::game::create_game_installation;
use std::fs;
use std::path::{Path, PathBuf};

pub struct SteamGameDiscovery;

impl SteamGameDiscovery {
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
}
