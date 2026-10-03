//! SMAPI releases and which game versions each one supports.
//!
//! Nothing here is maintained by this manager: releases come from SMAPI's
//! GitHub releases, and each release's game range is what SMAPI itself
//! declares (`MinimumGameVersion`, and `MaximumGameVersion` when it sets
//! one, in `src/SMAPI/Constants.cs` at that release's tag). The rule is
//! kept broad: a SMAPI version supports every game at least as new as its
//! minimum, up to its maximum if it has one.

use crate::version::SmapiVersion;
use serde::{Deserialize, Serialize};

/// One SMAPI release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmapiRelease {
    pub version: String,
    /// The release's git tag, where its Constants.cs is read.
    pub tag: String,
    pub published_at: Option<String>,
    pub prerelease: bool,
    /// The player installer archive (not the developer one).
    pub installer_url: String,
    /// SHA-256 of the installer as published with the release, if it was.
    pub sha256: Option<String>,
    /// The oldest game version this SMAPI supports, as SMAPI declares it.
    pub min_game: Option<String>,
    /// The newest game version it supports, when SMAPI sets one.
    pub max_game: Option<String>,
    pub notes_url: String,
}

/// How a SMAPI release relates to a game version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compatibility {
    Compatible,
    /// The game is older than this SMAPI's minimum.
    GameTooOld {
        minimum: String,
    },
    /// The game is newer than this SMAPI's maximum.
    GameTooNew {
        maximum: String,
    },
    /// The range or the game version is not known or not comparable.
    Unknown,
}

impl Compatibility {
    pub fn key(&self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::GameTooOld { .. } => "game_too_old",
            Self::GameTooNew { .. } => "game_too_new",
            Self::Unknown => "unknown",
        }
    }
}

pub fn compatibility(release: &SmapiRelease, game: Option<&str>) -> Compatibility {
    let Some(Ok(game)) = game.map(SmapiVersion::parse) else {
        return Compatibility::Unknown;
    };
    let Some(Ok(min)) = release.min_game.as_deref().map(SmapiVersion::parse) else {
        return Compatibility::Unknown;
    };
    if game < min {
        return Compatibility::GameTooOld {
            minimum: release.min_game.clone().unwrap_or_default(),
        };
    }
    if let Some(Ok(max)) = release.max_game.as_deref().map(SmapiVersion::parse) {
        // A maximum covers every build of that version, so compare by
        // major.minor.patch only.
        let cap = |v: &SmapiVersion| (v.major, v.minor, v.patch);
        if cap(&game) > cap(&max) {
            return Compatibility::GameTooNew {
                maximum: release.max_game.clone().unwrap_or_default(),
            };
        }
    }
    Compatibility::Compatible
}

/// Newest first, by version.
pub fn sort_releases(releases: &mut [SmapiRelease]) {
    releases.sort_by(|a, b| {
        match (
            SmapiVersion::parse(&a.version),
            SmapiVersion::parse(&b.version),
        ) {
            (Ok(x), Ok(y)) => y.cmp(&x),
            _ => b.version.cmp(&a.version),
        }
    });
}

/// The version to suggest: the newest stable release whose declared range
/// includes the game. When the game version is unknown, the release this
/// manager ships with, which it has tested, if listed; otherwise nothing.
pub fn recommend<'a>(
    releases: &'a [SmapiRelease],
    game: Option<&str>,
    tested: &str,
) -> Option<&'a SmapiRelease> {
    let mut stable: Vec<&SmapiRelease> = releases.iter().filter(|r| !r.prerelease).collect();
    stable.sort_by(|a, b| {
        match (
            SmapiVersion::parse(&a.version),
            SmapiVersion::parse(&b.version),
        ) {
            (Ok(x), Ok(y)) => y.cmp(&x),
            _ => b.version.cmp(&a.version),
        }
    });
    let known_game = game.is_some_and(|g| SmapiVersion::parse(g).is_ok());
    if !known_game {
        return stable.into_iter().find(|r| r.version == tested);
    }
    stable
        .into_iter()
        .find(|r| compatibility(r, game) == Compatibility::Compatible)
}

/// Reads `MinimumGameVersion` and `MaximumGameVersion` from SMAPI's
/// `Constants.cs`. A maximum of `null` means none.
pub fn parse_constants(text: &str) -> (Option<String>, Option<String>) {
    let read = |name: &str| -> Option<String> {
        let line = text.lines().find(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && t.contains(&format!("{name} {{ get; }}")) && t.contains('=')
        })?;
        let after = line.split_once('=')?.1;
        let start = after.find('"')? + 1;
        let end = after[start..].find('"')? + start;
        Some(after[start..end].to_string())
    };
    (read("MinimumGameVersion"), read("MaximumGameVersion"))
}

/// The installer archive's file name for a version, as SMAPI names it.
pub fn installer_file_name(version: &str) -> String {
    format!("SMAPI-{version}-installer.zip")
}

/// The release this manager ships with and has tested, used when nothing
/// can be read online. Its range is what SMAPI 4.1.10 declares.
pub fn builtin_release() -> SmapiRelease {
    SmapiRelease {
        version: crate::smapi::PINNED_SMAPI_VERSION.to_string(),
        tag: crate::smapi::PINNED_SMAPI_TAG.to_string(),
        published_at: None,
        prerelease: false,
        installer_url: crate::smapi::PINNED_SMAPI_URL.to_string(),
        sha256: Some(crate::smapi::PINNED_SMAPI_SHA256.to_string()),
        min_game: Some("1.6.14".to_string()),
        max_game: None,
        notes_url: format!(
            "https://github.com/Pathoschild/SMAPI/releases/tag/{}",
            crate::smapi::PINNED_SMAPI_TAG
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str, min: Option<&str>, max: Option<&str>) -> SmapiRelease {
        SmapiRelease {
            version: version.into(),
            tag: version.into(),
            published_at: None,
            prerelease: false,
            installer_url: String::new(),
            sha256: None,
            min_game: min.map(Into::into),
            max_game: max.map(Into::into),
            notes_url: String::new(),
        }
    }

    #[test]
    fn ranges_are_broad_minimum_and_optional_maximum() {
        let new = release("4.5.2", Some("1.6.14"), None);
        assert_eq!(
            compatibility(&new, Some("1.6.15")),
            Compatibility::Compatible
        );
        assert_eq!(
            compatibility(&new, Some("1.7.0")),
            Compatibility::Compatible
        );
        assert_eq!(
            compatibility(&new, Some("1.6.8")),
            Compatibility::GameTooOld {
                minimum: "1.6.14".into()
            }
        );
        let old = release("3.18.6", Some("1.5.6"), Some("1.5.6"));
        assert_eq!(
            compatibility(&old, Some("1.5.6")),
            Compatibility::Compatible
        );
        assert_eq!(
            compatibility(&old, Some("1.6.0")),
            Compatibility::GameTooNew {
                maximum: "1.5.6".into()
            }
        );
        assert_eq!(compatibility(&new, None), Compatibility::Unknown);
        assert_eq!(
            compatibility(&release("x", None, None), Some("1.6.15")),
            Compatibility::Unknown
        );
    }

    #[test]
    fn the_newest_compatible_stable_release_is_recommended() {
        let mut beta = release("4.6.0-beta", Some("1.6.14"), None);
        beta.prerelease = true;
        let list = vec![
            release("4.1.10", Some("1.6.14"), None),
            release("4.5.2", Some("1.6.14"), None),
            beta,
            release("3.18.6", Some("1.5.6"), Some("1.5.6")),
        ];
        assert_eq!(
            recommend(&list, Some("1.6.15"), "4.1.10").unwrap().version,
            "4.5.2"
        );
        assert_eq!(
            recommend(&list, Some("1.5.6"), "4.1.10").unwrap().version,
            "3.18.6"
        );
        // Unknown game version: the tested release, never a guess.
        assert_eq!(recommend(&list, None, "4.1.10").unwrap().version, "4.1.10");
        assert!(recommend(&list, Some("1.4.0"), "4.1.10").is_none());
    }

    #[test]
    fn constants_are_read_as_smapi_declares_them() {
        let text = r#"
    /// <summary>Uses <see cref="MinimumGameVersion"/> = "9.9".</summary>
    public static ISemanticVersion MinimumGameVersion { get; } = new GameVersion("1.6.14");
    /// <summary>The maximum supported version of Stardew Valley, if any.</summary>
    public static ISemanticVersion? MaximumGameVersion { get; } = null;
"#;
        assert_eq!(parse_constants(text), (Some("1.6.14".into()), None));
        let capped = r#"public static ISemanticVersion MinimumGameVersion { get; } = new GameVersion("1.5.6");
public static ISemanticVersion MaximumGameVersion { get; } = new GameVersion("1.5.6");"#;
        assert_eq!(
            parse_constants(capped),
            (Some("1.5.6".into()), Some("1.5.6".into()))
        );
    }
}
