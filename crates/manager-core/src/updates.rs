//! Mod updates: where a mod says it is published, what the update check
//! found, and which file of a GitHub release is the mod at a given version.
//!
//! The update check itself is SMAPI's (smapi.io), the same one SMAPI runs
//! when the game starts; this module only reads its answers. Choosing a
//! GitHub release file is deliberately strict: a file is chosen only when it
//! is a ZIP whose release or file name carries exactly the version the check
//! suggested, and, when a repository holds several mods, the mod's name too.
//! Whatever is chosen is still inspected like any other archive, and the
//! install is refused unless it contains the mod it was downloaded for.

use serde::{Deserialize, Serialize};

/// The largest release file the manager downloads, in bytes. The archive
/// inspector has its own limits; this one stops a download before it starts.
pub const MAX_RELEASE_ASSET_BYTES: u64 = 256 * 1024 * 1024;

/// One of a manifest's UpdateKeys, such as `Nexus:1915` or
/// `GitHub:owner/repo@Subkey`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateKey {
    /// The site as written, such as "Nexus" or "GitHub".
    pub site: String,
    pub id: String,
    /// The part after `@`, which picks one mod's files on a page that holds
    /// several.
    pub subkey: Option<String>,
}

impl UpdateKey {
    pub fn parse(raw: &str) -> Option<Self> {
        let (site, rest) = raw.trim().split_once(':')?;
        let site = site.trim();
        let (id, subkey) = match rest.split_once('@') {
            Some((id, sub)) => (id.trim(), Some(sub.trim().to_string())),
            None => (rest.trim(), None),
        };
        if site.is_empty() || id.is_empty() {
            return None;
        }
        Some(Self {
            site: site.to_string(),
            id: id.to_string(),
            subkey: subkey.filter(|s| !s.is_empty()),
        })
    }

    pub fn is_github(&self) -> bool {
        self.site.eq_ignore_ascii_case("github")
    }
}

/// A GitHub repository named as `owner/repo`, checked so it can only ever
/// become a path under api.github.com/repos/.
pub fn valid_github_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let ok = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s != "."
            && s != ".."
            && !s.to_lowercase().ends_with(".git")
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    ok(owner) && ok(name)
}

/// Where a mod's GitHub releases are, and who said so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubSource {
    pub repo: String,
    pub subkey: Option<String>,
    /// The mod's own manifest names it (a `GitHub:` update key), rather than
    /// only the community compatibility list.
    pub declared_by_mod: bool,
}

/// The repository a mod's own update keys name, else the one the
/// compatibility list gives for it.
pub fn github_source(update_keys: &[String], listed_repo: Option<&str>) -> Option<GitHubSource> {
    for key in update_keys.iter().filter_map(|k| UpdateKey::parse(k)) {
        if key.is_github() && valid_github_repo(&key.id) {
            return Some(GitHubSource {
                repo: key.id,
                subkey: key.subkey,
                declared_by_mod: true,
            });
        }
    }
    listed_repo
        .map(str::trim)
        .filter(|r| valid_github_repo(r))
        .map(|repo| GitHubSource {
            repo: repo.to_string(),
            subkey: None,
            declared_by_mod: false,
        })
}

/// A mod as the update check is asked about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModUpdateQuery {
    pub unique_id: String,
    pub installed_version: String,
    pub update_keys: Vec<String>,
}

/// What the update check said about one mod.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModUpdateInfo {
    pub unique_id: String,
    /// The version to update to, when there is a newer one.
    pub suggested_version: Option<String>,
    /// The page the update is published on.
    pub suggested_url: Option<String>,
    /// The newest version known on any site, newer or not.
    pub latest_version: Option<String>,
    pub name: Option<String>,
    /// The GitHub repository the compatibility list gives, if any.
    pub github_repo: Option<String>,
    /// The compatibility list's status, such as "Ok", "Broken" or
    /// "Obsolete", and its summary.
    pub compatibility_status: Option<String>,
    pub compatibility_summary: Option<String>,
    /// What the check could not read, as the check words it.
    pub errors: Vec<String>,
}

/// One release as GitHub lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub tag: String,
    pub name: Option<String>,
    pub html_url: String,
    pub draft: bool,
    pub prerelease: bool,
    pub published_at: Option<String>,
    pub assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub size: u64,
    pub download_url: String,
    /// The SHA-256 GitHub records for the file, where it has one.
    pub sha256: Option<String>,
}

/// The release file chosen for a version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChosenAsset {
    pub repo: String,
    pub tag: String,
    pub release_url: String,
    pub asset: GitHubAsset,
}

/// Why no release file was chosen, in words for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoAsset {
    /// No release carries this version in a ZIP file.
    NoMatchingRelease,
    /// Several files could be this mod at this version.
    Ambiguous(Vec<String>),
    /// The repository holds several mods' releases and none is named for
    /// this one.
    NotNamedForMod,
    TooLarge {
        name: String,
        size: u64,
    },
}

impl NoAsset {
    pub fn describe(&self, version: &str) -> String {
        match self {
            NoAsset::NoMatchingRelease => {
                format!("No GitHub release has a ZIP file for version {version}.")
            }
            NoAsset::Ambiguous(names) => format!(
                "More than one GitHub release file could be version {version}: {}.",
                names.join(", ")
            ),
            NoAsset::NotNamedForMod => format!(
                "A GitHub release for version {version} exists, but it is not named for this mod, so it may be another mod's."
            ),
            NoAsset::TooLarge { name, size } => format!(
                "The GitHub release file {name} is {} MB, more than the manager downloads.",
                size / (1024 * 1024)
            ),
        }
    }
}

/// Lower case letters and digits only: "Project Fluent", "project-fluent"
/// and "ProjectFluent" all read "projectfluent".
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// The ways a version is written that mean the same version: "2.0" and
/// "2.0.0" are one version, as SMAPI compares them.
fn version_spellings(version: &str) -> Vec<String> {
    let (core, rest) = match version.find(['-', '+']) {
        Some(i) => version.split_at(i),
        None => (version, ""),
    };
    let mut parts: Vec<&str> = core.split('.').collect();
    let mut spellings = vec![version.to_string()];
    if parts
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        while parts.len() > 2 && parts.last().is_some_and(|p| p.bytes().all(|b| b == b'0')) {
            parts.pop();
            spellings.push(format!("{}{rest}", parts.join(".")));
        }
        let mut longer: Vec<&str> = core.split('.').collect();
        while longer.len() < 3 {
            longer.push("0");
            spellings.push(format!("{}{rest}", longer.join(".")));
        }
    }
    spellings.dedup();
    spellings
}

/// Words that may follow a version without making it another version, as
/// in "Mod 1.0.0-win.zip".
const NEUTRAL_SUFFIXES: &[&str] = &[
    "win", "windows", "linux", "mac", "macos", "osx", "release", "final",
];

/// Whether `version` appears in `text` as a whole version: "0.6.1" is in
/// "v0.6.1" and "StardewUI.0.6.1.zip", but not in "0.6.10", "10.6.1",
/// "0.6.1a" or "0.6.1-dev". "2.0" and "2.0.0" are the same version.
pub fn mentions_version(text: &str, version: &str) -> bool {
    let version = version.trim();
    if version.is_empty() {
        return false;
    }
    let text_lower = text.to_lowercase();
    version_spellings(&version.to_lowercase())
        .iter()
        .any(|spelling| mentions_spelling(&text_lower, spelling))
}

fn mentions_spelling(text_lower: &str, version_lower: &str) -> bool {
    let bytes = text_lower.as_bytes();
    let mut from = 0;
    while let Some(offset) = text_lower[from..].find(version_lower) {
        let start = from + offset;
        let end = start + version_lower.len();
        let before_ok = start == 0 || {
            let b = bytes[start - 1];
            !b.is_ascii_digit() && !(b == b'.' && start >= 2 && bytes[start - 2].is_ascii_digit())
        };
        let after_ok = end == bytes.len() || {
            let a = bytes[end];
            let next_digit = end + 1 < bytes.len() && bytes[end + 1].is_ascii_digit();
            if a.is_ascii_alphanumeric() {
                // "1.2.3a" and "1.2.30" are other versions.
                false
            } else if a == b'.' {
                !next_digit
            } else if a == b'-' {
                // "1.0.0-beta" or "1.0.0-dev" is not "1.0.0", though
                // "1.0.0-win" is.
                let word: String = text_lower[end + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                NEUTRAL_SUFFIXES.contains(&word.as_str())
            } else {
                true
            }
        };
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// The names a file or tag is written with, one per word: "release/project-
/// fluent/2.0.0" names "projectfluent", and "StardewUI-v0.6.1.zip" names
/// "stardewui". A word's hyphenated parts belong together, apart from a
/// version or platform at its end, so "Fluent" is not "Project Fluent".
fn named_words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter_map(|word| {
            let mut parts: Vec<&str> = word.split('-').filter(|p| !p.is_empty()).collect();
            while parts.len() > 1 && parts.last().is_some_and(|p| is_version_or_platform(p)) {
                parts.pop();
            }
            let key = squash(&parts.concat());
            (!key.is_empty()).then_some(key)
        })
        .collect()
}

fn is_version_or_platform(part: &str) -> bool {
    let lower = part.to_lowercase();
    let digits = lower.strip_prefix('v').unwrap_or(&lower);
    digits.starts_with(|c: char| c.is_ascii_digit()) || NEUTRAL_SUFFIXES.contains(&lower.as_str())
}

/// The names a mod may be published under: its name, the last part of its
/// UniqueID, and the update key's subkey.
fn mod_tokens(name: &str, unique_id: &str, subkey: Option<&str>) -> Vec<String> {
    let mut tokens = vec![squash(name)];
    if let Some(last) = unique_id.rsplit('.').next() {
        tokens.push(squash(last));
    }
    if let Some(sub) = subkey {
        tokens.push(squash(sub));
    }
    tokens.retain(|t| t.len() >= 3);
    tokens.dedup();
    tokens
}

/// Files beside a release's mod that are not the mod: its source code or
/// debugging symbols.
fn is_side_file(words: &[String]) -> bool {
    words
        .iter()
        .any(|w| matches!(w.as_str(), "source" | "sources" | "src" | "symbols" | "pdb"))
}

/// The release file that is this mod at `version`, if exactly one is.
///
/// A candidate is a ZIP in a published (non-draft) release, a prerelease
/// only for a prerelease version, whose tag,
/// release name or file name carries the version. With a subkey only files
/// named for it count. When the mod's own manifest names the repository and
/// there is one candidate, it is the one; otherwise the candidate must be
/// named for the mod, since a repository can hold several mods' releases.
pub fn choose_release_asset(
    source: &GitHubSource,
    releases: &[GitHubRelease],
    version: &str,
    mod_name: &str,
    unique_id: &str,
) -> Result<ChosenAsset, NoAsset> {
    let tokens = mod_tokens(mod_name, unique_id, source.subkey.as_deref());
    let subkey = source
        .subkey
        .as_deref()
        .map(squash)
        .filter(|s| !s.is_empty());
    let wants_prerelease = version.contains('-');
    let mut candidates: Vec<(&GitHubRelease, &GitHubAsset, bool)> = Vec::new();
    for release in releases
        .iter()
        .filter(|r| !r.draft && (wants_prerelease || !r.prerelease))
    {
        let release_text = format!("{} {}", release.tag, release.name.as_deref().unwrap_or(""));
        let release_has_version = mentions_version(&release_text, version);
        for asset in &release.assets {
            if !asset.name.to_lowercase().ends_with(".zip") {
                continue;
            }
            if !release_has_version && !mentions_version(&asset.name, version) {
                continue;
            }
            let asset_words =
                named_words(asset.name.trim_end_matches(".zip").trim_end_matches(".ZIP"));
            if is_side_file(&asset_words) {
                continue;
            }
            let mut words = named_words(&release.tag);
            words.extend(asset_words);
            if let Some(sub) = &subkey {
                if !words.contains(sub) {
                    continue;
                }
            }
            let named_for_mod = tokens.iter().any(|t| words.contains(t));
            candidates.push((release, asset, named_for_mod));
        }
    }
    if candidates.is_empty() {
        return Err(NoAsset::NoMatchingRelease);
    }
    let named: Vec<_> = candidates.iter().filter(|(_, _, n)| *n).collect();
    let chosen = if candidates.len() == 1 && (source.declared_by_mod || candidates[0].2) {
        &candidates[0]
    } else if named.len() == 1 {
        named[0]
    } else if named.is_empty() {
        if candidates.len() == 1 {
            return Err(NoAsset::NotNamedForMod);
        }
        return Err(NoAsset::Ambiguous(
            candidates.iter().map(|(_, a, _)| a.name.clone()).collect(),
        ));
    } else {
        return Err(NoAsset::Ambiguous(
            named.iter().map(|(_, a, _)| a.name.clone()).collect(),
        ));
    };
    let (release, asset, _) = chosen;
    if asset.size > MAX_RELEASE_ASSET_BYTES {
        return Err(NoAsset::TooLarge {
            name: asset.name.clone(),
            size: asset.size,
        });
    }
    Ok(ChosenAsset {
        repo: source.repo.clone(),
        tag: release.tag.clone(),
        release_url: release.html_url.clone(),
        asset: (*asset).clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.into(),
            size: 1000,
            download_url: format!("https://github.com/x/y/releases/download/t/{name}"),
            sha256: None,
        }
    }

    fn release(tag: &str, assets: &[&str]) -> GitHubRelease {
        GitHubRelease {
            tag: tag.into(),
            name: None,
            html_url: format!("https://github.com/x/y/releases/tag/{tag}"),
            draft: false,
            prerelease: false,
            published_at: None,
            assets: assets.iter().map(|a| asset(a)).collect(),
        }
    }

    fn declared(repo: &str) -> GitHubSource {
        GitHubSource {
            repo: repo.into(),
            subkey: None,
            declared_by_mod: true,
        }
    }

    fn listed(repo: &str) -> GitHubSource {
        GitHubSource {
            declared_by_mod: false,
            ..declared(repo)
        }
    }

    #[test]
    fn update_keys_parse_with_and_without_subkeys() {
        let key = UpdateKey::parse("GitHub:Shockah/Stardew-Valley-Mods@Kokoro").unwrap();
        assert!(key.is_github());
        assert_eq!(key.id, "Shockah/Stardew-Valley-Mods");
        assert_eq!(key.subkey.as_deref(), Some("Kokoro"));
        let key = UpdateKey::parse(" Nexus : 1915 ").unwrap();
        assert_eq!((key.site.as_str(), key.id.as_str()), ("Nexus", "1915"));
        assert_eq!(key.subkey, None);
        assert_eq!(UpdateKey::parse("Nexus"), None);
        assert_eq!(UpdateKey::parse(":1"), None);
        assert_eq!(UpdateKey::parse("Nexus:"), None);
    }

    #[test]
    fn only_plain_owner_and_repo_names_are_repositories() {
        assert!(valid_github_repo("focustense/StardewUI"));
        assert!(valid_github_repo("Shockah/Stardew-Valley-Mods"));
        assert!(!valid_github_repo("focustense"));
        assert!(!valid_github_repo("a/b/c"));
        assert!(!valid_github_repo("../etc"));
        assert!(!valid_github_repo("a/.."));
        assert!(!valid_github_repo("a/b?x=1"));
        assert!(!valid_github_repo("a b/c"));
        assert!(!valid_github_repo("a/b.git"));
    }

    #[test]
    fn the_mods_own_key_wins_over_the_compatibility_list() {
        let keys = vec!["Nexus:28870".into(), "GitHub:focustense/StardewUI".into()];
        let found = github_source(&keys, Some("someone/else")).unwrap();
        assert_eq!(found.repo, "focustense/StardewUI");
        assert!(found.declared_by_mod);
        let found =
            github_source(&["Nexus:1".into()], Some("Shockah/Stardew-Valley-Mods")).unwrap();
        assert!(!found.declared_by_mod);
        assert_eq!(github_source(&["Nexus:1".into()], Some("not a repo")), None);
        assert_eq!(github_source(&[], None), None);
    }

    #[test]
    fn versions_match_whole_versions_only() {
        assert!(mentions_version("v0.6.1", "0.6.1"));
        assert!(mentions_version("StardewUI.0.6.1.zip", "0.6.1"));
        assert!(mentions_version("release/kokoro/3.0.0", "3.0.0"));
        assert!(mentions_version("BetterGameMenu-0.5.10", "0.5.10"));
        assert!(!mentions_version("BetterGameMenu-0.5.10", "0.5.1"));
        assert!(!mentions_version("10.6.1", "0.6.1"));
        assert!(!mentions_version("1.0.6.1", "0.6.1"));
        assert!(!mentions_version("Mod-1.0.0-beta.zip", "1.0.0"));
        assert!(mentions_version("Mod-1.0.0-beta.zip", "1.0.0-beta"));
        assert!(mentions_version("Mod 1.0.0-win.zip", "1.0.0"));
        assert!(!mentions_version("anything", ""));
        // The same version written two ways.
        assert!(mentions_version("v2.0.0", "2.0"));
        assert!(mentions_version("Thing.2.0.zip", "2.0.0"));
        assert!(!mentions_version("v2.0.1", "2.0"));
        // Other versions that start the same way.
        assert!(!mentions_version("Thing-1.2.3a.zip", "1.2.3"));
        assert!(!mentions_version("v1.2.3-dev", "1.2.3"));
        assert!(!mentions_version("v1.2.3-b1", "1.2.3"));
        assert!(mentions_version("v1.2.3+build.5", "1.2.3"));
    }

    #[test]
    fn names_are_whole_words() {
        assert_eq!(
            named_words("release/project-fluent/2.0.0"),
            ["release", "projectfluent", "2", "0", "0"]
        );
        assert_eq!(named_words("StardewUI-v0.6.1"), ["stardewui", "6", "1"]);
        assert_eq!(
            named_words("BetterGameMenu-0.5.10"),
            ["bettergamemenu", "5", "10"]
        );
        // "Fluent" is not named by Project Fluent's files.
        let releases = vec![release(
            "release/project-fluent/1.0.0",
            &["ProjectFluent.1.0.0.zip"],
        )];
        assert_eq!(
            choose_release_asset(
                &listed("Shockah/Mods"),
                &releases,
                "1.0.0",
                "Fluent",
                "x.Fluent"
            ),
            Err(NoAsset::NotNamedForMod)
        );
        // Nor does a subkey match part of a word.
        let releases = vec![release("v1.0.0", &["BuildUI.1.0.0.zip", "Other.1.0.0.zip"])];
        let source = GitHubSource {
            subkey: Some("UI".into()),
            ..declared("a/b")
        };
        assert_eq!(
            choose_release_asset(&source, &releases, "1.0.0", "UI", "a.UI"),
            Err(NoAsset::NoMatchingRelease)
        );
    }

    #[test]
    fn source_files_and_prereleases_are_left_out() {
        let releases = vec![release("v1.2.3", &["Thing.1.2.3.zip", "Thing-source.zip"])];
        let chosen =
            choose_release_asset(&declared("a/b"), &releases, "1.2.3", "Thing", "a.Thing").unwrap();
        assert_eq!(chosen.asset.name, "Thing.1.2.3.zip");
        let mut pre = release("v1.2.3", &["Thing.1.2.3.zip"]);
        pre.prerelease = true;
        assert_eq!(
            choose_release_asset(
                &declared("a/b"),
                &[pre.clone()],
                "1.2.3",
                "Thing",
                "a.Thing"
            ),
            Err(NoAsset::NoMatchingRelease)
        );
        pre.tag = "v1.2.3-beta.1".into();
        pre.assets = vec![asset("Thing.1.2.3-beta.1.zip")];
        assert!(
            choose_release_asset(&declared("a/b"), &[pre], "1.2.3-beta.1", "Thing", "a.Thing")
                .is_ok()
        );
    }

    #[test]
    fn a_single_repository_release_is_chosen_by_version() {
        // focustense/StardewUI as published: a ZIP beside other files.
        let releases = vec![
            GitHubRelease {
                assets: vec![
                    asset("StardewUI.0.6.1.zip"),
                    asset("StardewUI.VisualStudio.vsix"),
                    asset("StarML_by_focustense.xml"),
                ],
                ..release("v0.6.1", &[])
            },
            release("v0.6.0", &["StardewUI.0.6.0.zip"]),
        ];
        let chosen = choose_release_asset(
            &declared("focustense/StardewUI"),
            &releases,
            "0.6.1",
            "Stardew UI",
            "focustense.StardewUI",
        )
        .unwrap();
        assert_eq!(chosen.tag, "v0.6.1");
        assert_eq!(chosen.asset.name, "StardewUI.0.6.1.zip");
    }

    #[test]
    fn a_shared_repository_release_must_be_named_for_the_mod() {
        // Shockah/Stardew-Valley-Mods holds many mods' releases.
        let releases = vec![
            release("release/project-fluent/2.0.0", &["ProjectFluent.2.0.0.zip"]),
            release("release/kokoro/3.0.0", &["Kokoro.3.0.0.zip"]),
            release("release/other/3.0.0", &["Other.3.0.0.zip"]),
        ];
        let source = listed("Shockah/Stardew-Valley-Mods");
        let chosen =
            choose_release_asset(&source, &releases, "3.0.0", "Kokoro", "Shockah.Kokoro").unwrap();
        assert_eq!(chosen.asset.name, "Kokoro.3.0.0.zip");
        let chosen = choose_release_asset(
            &source,
            &releases,
            "2.0.0",
            "Project Fluent",
            "Shockah.ProjectFluent",
        )
        .unwrap();
        assert_eq!(chosen.tag, "release/project-fluent/2.0.0");
        // Only someone else's release has the version.
        assert_eq!(
            choose_release_asset(&source, &releases, "2.0.0", "Kokoro", "Shockah.Kokoro"),
            Err(NoAsset::NotNamedForMod)
        );
    }

    #[test]
    fn the_subkey_picks_one_mods_files() {
        let releases = vec![release("3.0.0", &["Kokoro.3.0.0.zip", "Other.3.0.0.zip"])];
        let source = GitHubSource {
            subkey: Some("Kokoro".into()),
            ..declared("Shockah/Stardew-Valley-Mods")
        };
        let chosen =
            choose_release_asset(&source, &releases, "3.0.0", "Kokoro", "Shockah.Kokoro").unwrap();
        assert_eq!(chosen.asset.name, "Kokoro.3.0.0.zip");
    }

    #[test]
    fn nothing_is_guessed() {
        // A version the repository never released.
        let releases = vec![release(
            "BetterGameMenu-0.5.10",
            &["BetterGameMenu.0.5.10.zip"],
        )];
        assert_eq!(
            choose_release_asset(
                &listed("KhloeLeclair/StardewMods"),
                &releases,
                "1.0.1",
                "Better Game Menu",
                "leclair.bettergamemenu"
            ),
            Err(NoAsset::NoMatchingRelease)
        );
        // Two files, neither named for the mod.
        let releases = vec![release("v1.0.0", &["windows.zip", "linux.zip"])];
        assert!(matches!(
            choose_release_asset(&declared("a/b"), &releases, "1.0.0", "Thing", "a.Thing"),
            Err(NoAsset::Ambiguous(_))
        ));
        // Drafts and non-ZIP files are never candidates.
        let mut draft = release("v1.0.0", &["Thing.1.0.0.zip"]);
        draft.draft = true;
        let releases = vec![draft, release("v1.0.0", &["Thing.1.0.0.7z"])];
        assert_eq!(
            choose_release_asset(&declared("a/b"), &releases, "1.0.0", "Thing", "a.Thing"),
            Err(NoAsset::NoMatchingRelease)
        );
    }

    #[test]
    fn oversized_files_are_not_downloaded() {
        let mut big = release("v1.0.0", &["Thing.1.0.0.zip"]);
        big.assets[0].size = MAX_RELEASE_ASSET_BYTES + 1;
        assert!(matches!(
            choose_release_asset(&declared("a/b"), &[big], "1.0.0", "Thing", "a.Thing"),
            Err(NoAsset::TooLarge { .. })
        ));
    }
}
