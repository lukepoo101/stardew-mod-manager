//! Nexus Mods: where a mod says it is published there, the `nxm://` links
//! the site's "Mod manager download" button opens, and which of a mod's
//! files is the mod at a given version.
//!
//! Everything here is pure. Talking to Nexus is the job of a port in the
//! application layer, and nothing here holds or logs an API key: the key and
//! expiry an `nxm://` link carries are kept out of its `Debug` output.
//!
//! Choosing a file is as strict as choosing a GitHub release file (see
//! `crate::updates::choose_release_asset`): a file is chosen only when its
//! version is exactly the one asked for, it is a ZIP, and it is not an old or
//! archived file. Whatever is chosen is still inspected like any other
//! archive, and the install is refused unless it holds the mod it was
//! downloaded for.

use crate::updates::{is_side_file, named_words, squash, UpdateKey, MAX_RELEASE_ASSET_BYTES};
use crate::version::SmapiVersion;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Nexus's name for Stardew Valley, in its URLs and its API.
pub const STARDEW_GAME_DOMAIN: &str = "stardewvalley";

/// The longest `nxm://` link read. Real ones are well under 300 characters.
const MAX_LINK_LEN: usize = 2048;

/// The longest download key accepted from a link.
const MAX_KEY_LEN: usize = 128;

/// A Nexus mod page's address.
pub fn mod_page_url(mod_id: u64) -> String {
    format!("https://www.nexusmods.com/{STARDEW_GAME_DOMAIN}/mods/{mod_id}")
}

/// The Nexus mod a manifest's update keys name (`Nexus:1915`, or
/// `Nexus:1915@Subkey` for one of several mods on a page), and its subkey.
pub fn nexus_source(update_keys: &[String]) -> Option<NexusSource> {
    update_keys
        .iter()
        .filter_map(|k| UpdateKey::parse(k))
        .filter(|k| k.site.eq_ignore_ascii_case("nexus"))
        .find_map(|k| {
            let mod_id = parse_id(&k.id)?;
            Some(NexusSource {
                mod_id,
                subkey: k.subkey,
            })
        })
}

/// A mod page on Nexus, as a manifest's update key names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NexusSource {
    pub mod_id: u64,
    pub subkey: Option<String>,
}

/// A positive whole number of at most 12 digits: a Nexus mod, file or user
/// id. Nothing else, so an id can only ever become a path segment.
fn parse_id(text: &str) -> Option<u64> {
    let text = text.trim();
    if text.is_empty() || text.len() > 12 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|id| *id > 0)
}

/// A link from Nexus's "Mod manager download" button:
/// `nxm://stardewvalley/mods/{mod}/files/{file}?key=...&expires=...&user_id=...`.
///
/// The key and expiry let someone without Premium download that one file
/// through the API for a short while. They are never logged: `Debug` leaves
/// them out.
#[derive(Clone, PartialEq, Eq)]
pub struct NxmLink {
    pub game: String,
    pub mod_id: u64,
    pub file_id: u64,
    pub key: Option<String>,
    /// When the key stops working, in seconds since 1970.
    pub expires: Option<u64>,
    pub user_id: Option<u64>,
}

impl fmt::Debug for NxmLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NxmLink")
            .field("game", &self.game)
            .field("mod_id", &self.mod_id)
            .field("file_id", &self.file_id)
            .field("key", &self.key.as_ref().map(|_| "<redacted>"))
            .field("expires", &self.expires)
            .finish()
    }
}

/// Why an `nxm://` link was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NxmLinkError {
    /// It is not an `nxm://` link at all.
    NotNxm,
    /// It is for another game's mods.
    OtherGame(String),
    /// A link to a collection, which the manager does not install.
    Collection,
    /// It is an `nxm://` link, but not one to a mod's file.
    Malformed,
}

impl NxmLinkError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotNxm => "NXM_LINK_NOT_NXM",
            Self::OtherGame(_) => "NXM_LINK_OTHER_GAME",
            Self::Collection => "NXM_LINK_COLLECTION",
            Self::Malformed => "NXM_LINK_MALFORMED",
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::NotNxm => "That is not a Nexus Mods download link.".into(),
            Self::OtherGame(game) => format!(
                "That Nexus Mods link is for {game}, not Stardew Valley, so it was ignored."
            ),
            Self::Collection => {
                "That link is to a Nexus Mods collection. Collections cannot be installed from Nexus yet; install their mods one at a time.".into()
            }
            Self::Malformed => {
                "That Nexus Mods link is not one to a mod's file, so it was ignored.".into()
            }
        }
    }
}

impl NxmLink {
    /// Reads a link, for Stardew Valley only. Anything that is not exactly a
    /// link to one mod file is refused rather than guessed at.
    pub fn parse(raw: &str) -> Result<Self, NxmLinkError> {
        let raw = raw.trim();
        if raw.len() > MAX_LINK_LEN {
            return Err(NxmLinkError::Malformed);
        }
        let rest = match raw.split_once("://") {
            Some((scheme, rest)) if scheme.eq_ignore_ascii_case("nxm") => rest,
            _ => return Err(NxmLinkError::NotNxm),
        };
        let (path, query) = match rest.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (rest, None),
        };
        // A fragment is never part of a real link; one is not looked at.
        let query = query.map(|q| q.split('#').next().unwrap_or(""));
        let path = path.split('#').next().unwrap_or("");
        let mut parts = path.split('/');
        let game = parts.next().unwrap_or("").to_ascii_lowercase();
        if game.is_empty()
            || !game
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(NxmLinkError::Malformed);
        }
        let segments: Vec<&str> = parts.filter(|p| !p.is_empty()).collect();
        if segments
            .first()
            .is_some_and(|s| s.eq_ignore_ascii_case("collections"))
        {
            return if game == STARDEW_GAME_DOMAIN {
                Err(NxmLinkError::Collection)
            } else {
                Err(NxmLinkError::OtherGame(game))
            };
        }
        if game != STARDEW_GAME_DOMAIN {
            return Err(NxmLinkError::OtherGame(game));
        }
        let (mod_id, file_id) = match segments.as_slice() {
            [mods, mod_id, files, file_id]
                if mods.eq_ignore_ascii_case("mods") && files.eq_ignore_ascii_case("files") =>
            {
                (
                    parse_id(mod_id).ok_or(NxmLinkError::Malformed)?,
                    parse_id(file_id).ok_or(NxmLinkError::Malformed)?,
                )
            }
            _ => return Err(NxmLinkError::Malformed),
        };
        let mut link = NxmLink {
            game,
            mod_id,
            file_id,
            key: None,
            expires: None,
            user_id: None,
        };
        for pair in query.unwrap_or("").split('&').filter(|p| !p.is_empty()) {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            match name {
                "key" => {
                    let ok = !value.is_empty()
                        && value.len() <= MAX_KEY_LEN
                        && value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
                    if !ok {
                        return Err(NxmLinkError::Malformed);
                    }
                    link.key = Some(value.to_string());
                }
                "expires" => {
                    link.expires = Some(parse_id(value).ok_or(NxmLinkError::Malformed)?);
                }
                "user_id" => link.user_id = parse_id(value),
                // Anything else Nexus may add is not needed.
                _ => {}
            }
        }
        // A key is only any use with its expiry.
        if link.key.is_some() != link.expires.is_some() {
            return Err(NxmLinkError::Malformed);
        }
        Ok(link)
    }

    /// Whether the link's key has stopped working at `now` (seconds since
    /// 1970). A link without a key does not expire; it only works for
    /// Premium members.
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires.is_some_and(|expires| expires <= now)
    }

    /// The mod's page, open at its files.
    pub fn page_url(&self) -> String {
        format!("{}?tab=files", mod_page_url(self.mod_id))
    }
}

/// Which list a file is in on its mod's Files tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NexusFileCategory {
    Main,
    Update,
    Optional,
    OldVersion,
    Miscellaneous,
    Archived,
    Other,
}

impl NexusFileCategory {
    /// From the API's `category_name` ("MAIN", "OLD_VERSION", ...).
    pub fn from_api(name: &str) -> Self {
        match name.trim().to_ascii_uppercase().as_str() {
            "MAIN" => Self::Main,
            "UPDATE" => Self::Update,
            "OPTIONAL" => Self::Optional,
            "OLD_VERSION" => Self::OldVersion,
            "MISCELLANEOUS" => Self::Miscellaneous,
            "ARCHIVED" => Self::Archived,
            _ => Self::Other,
        }
    }

    /// Files the author still offers as the mod.
    fn is_current(self) -> bool {
        matches!(self, Self::Main | Self::Update | Self::Optional)
    }
}

/// One of a mod's files as Nexus lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NexusFile {
    pub file_id: u64,
    /// The name shown on the Files tab.
    pub name: String,
    pub version: Option<String>,
    pub category: NexusFileCategory,
    /// The file's name as it downloads.
    pub file_name: String,
    pub size_bytes: u64,
    /// When it was uploaded, in seconds since 1970.
    pub uploaded_at: Option<i64>,
}

impl NexusFile {
    pub fn is_zip(&self) -> bool {
        self.file_name.to_ascii_lowercase().ends_with(".zip")
    }
}

/// Why no file of a mod was chosen for a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoNexusFile {
    /// No current file is that version.
    NoMatchingFile,
    /// Files of that version exist, but none is a ZIP; the manager installs
    /// ZIPs only.
    NotZip(Vec<String>),
    /// Several files are that version and nothing tells them apart.
    Ambiguous(Vec<String>),
    TooLarge {
        name: String,
        size: u64,
    },
}

impl NoNexusFile {
    pub fn describe(&self, version: &str) -> String {
        match self {
            Self::NoMatchingFile => format!(
                "None of the mod's current files on Nexus is version {version}."
            ),
            Self::NotZip(names) => format!(
                "The mod's {version} file on Nexus is not a ZIP ({}), and the manager installs ZIP archives only.",
                names.join(", ")
            ),
            Self::Ambiguous(names) => format!(
                "Several of the mod's files on Nexus are version {version} ({}), so the manager will not guess which one is this mod.",
                names.join(", ")
            ),
            Self::TooLarge { name, size } => format!(
                "{name} is {} MB, more than the manager downloads.",
                size / (1024 * 1024)
            ),
        }
    }
}

fn same_version(a: &str, b: &str) -> bool {
    match (SmapiVersion::parse(a), SmapiVersion::parse(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

/// The file that is this mod at `version`, if exactly one is.
///
/// A candidate is a main, update or optional file (not an old, archived or
/// miscellaneous one) whose version is exactly `version` ("2.0" and "2.0.0"
/// are one version) and which is not source code or debugging symbols. With
/// a subkey only files named for it count. Of several candidates, the one
/// main file is chosen; otherwise nothing is.
pub fn choose_nexus_file<'a>(
    files: &'a [NexusFile],
    version: &str,
    subkey: Option<&str>,
) -> Result<&'a NexusFile, NoNexusFile> {
    let subkey = subkey.map(squash).filter(|s| !s.is_empty());
    let candidates: Vec<&NexusFile> = files
        .iter()
        .filter(|f| f.category.is_current())
        .filter(|f| {
            f.version
                .as_deref()
                .is_some_and(|v| same_version(v, version))
        })
        .filter(|f| {
            let mut words = named_words(&f.name);
            words.extend(named_words(&f.file_name));
            if is_side_file(&words) {
                return false;
            }
            match &subkey {
                Some(sub) => words.contains(sub),
                None => true,
            }
        })
        .collect();
    let zips: Vec<&NexusFile> = candidates.iter().copied().filter(|f| f.is_zip()).collect();
    if zips.is_empty() {
        return Err(if candidates.is_empty() {
            NoNexusFile::NoMatchingFile
        } else {
            NoNexusFile::NotZip(candidates.iter().map(|f| f.file_name.clone()).collect())
        });
    }
    let chosen = if zips.len() == 1 {
        zips[0]
    } else {
        let main: Vec<&NexusFile> = zips
            .iter()
            .copied()
            .filter(|f| f.category == NexusFileCategory::Main)
            .collect();
        if main.len() == 1 {
            main[0]
        } else {
            return Err(NoNexusFile::Ambiguous(
                zips.iter().map(|f| f.file_name.clone()).collect(),
            ));
        }
    };
    if chosen.size_bytes > MAX_RELEASE_ASSET_BYTES {
        return Err(NoNexusFile::TooLarge {
            name: chosen.file_name.clone(),
            size: chosen.size_bytes,
        });
    }
    Ok(chosen)
}

/// Whether `text` is an MD5 digest written in hex.
pub fn valid_md5(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str =
        "nxm://stardewvalley/mods/541/files/12345?key=AbC-12_x&expires=1800000000&user_id=99";

    #[test]
    fn reads_a_mod_manager_download_link() {
        let link = NxmLink::parse(LINK).unwrap();
        assert_eq!(link.game, "stardewvalley");
        assert_eq!((link.mod_id, link.file_id), (541, 12345));
        assert_eq!(link.key.as_deref(), Some("AbC-12_x"));
        assert_eq!(link.expires, Some(1_800_000_000));
        assert_eq!(link.user_id, Some(99));
        assert!(!link.is_expired(1_799_999_999));
        assert!(link.is_expired(1_800_000_000));
        assert_eq!(
            link.page_url(),
            "https://www.nexusmods.com/stardewvalley/mods/541?tab=files"
        );
        // Case of the scheme, game and path words does not matter.
        assert!(NxmLink::parse("NXM://StardewValley/Mods/1/Files/2").is_ok());
    }

    #[test]
    fn the_key_never_appears_in_debug_output() {
        let link = NxmLink::parse(LINK).unwrap();
        let shown = format!("{link:?}");
        assert!(!shown.contains("AbC-12_x"), "{shown}");
        assert!(shown.contains("<redacted>"));
    }

    #[test]
    fn a_link_without_a_key_is_for_premium_members() {
        let link = NxmLink::parse("nxm://stardewvalley/mods/1/files/2").unwrap();
        assert_eq!((link.key.as_deref(), link.expires), (None, None));
        assert!(!link.is_expired(u64::MAX));
    }

    #[test]
    fn anything_else_is_refused() {
        use NxmLinkError::*;
        let cases = [
            ("https://www.nexusmods.com/stardewvalley/mods/1", NotNxm),
            ("stardewvalley/mods/1/files/2", NotNxm),
            (
                "nxm://skyrimspecialedition/mods/1/files/2",
                OtherGame("skyrimspecialedition".into()),
            ),
            (
                "nxm://stardewvalley/collections/abc/revisions/3",
                Collection,
            ),
            (
                "nxm://skyrim/collections/abc/revisions/3",
                OtherGame("skyrim".into()),
            ),
            ("nxm://stardewvalley/mods/1", Malformed),
            ("nxm://stardewvalley/mods/1/files/x", Malformed),
            ("nxm://stardewvalley/mods/0/files/2", Malformed),
            ("nxm://stardewvalley/mods/1/files/2/more", Malformed),
            (
                "nxm://stardewvalley/mods/1/files/2?key=a b&expires=1",
                Malformed,
            ),
            ("nxm://stardewvalley/mods/1/files/2?key=abc", Malformed),
            ("nxm://stardewvalley/mods/1/files/2?expires=1", Malformed),
            (
                "nxm://stardewvalley/mods/1/files/2?key=a&expires=soon",
                Malformed,
            ),
            ("nxm://stardew valley/mods/1/files/2", Malformed),
            ("nxm://../mods/1/files/2", Malformed),
            ("nxm://stardewvalley/mods/1234567890123/files/2", Malformed),
        ];
        for (raw, why) in cases {
            assert_eq!(NxmLink::parse(raw), Err(why), "{raw}");
        }
        let long = format!("nxm://stardewvalley/mods/1/files/2?x={}", "a".repeat(3000));
        assert_eq!(NxmLink::parse(&long), Err(Malformed));
    }

    #[test]
    fn finds_the_nexus_page_in_update_keys() {
        let keys = vec!["GitHub:a/b".into(), "Nexus:1915@Fluent".into()];
        assert_eq!(
            nexus_source(&keys),
            Some(NexusSource {
                mod_id: 1915,
                subkey: Some("Fluent".into())
            })
        );
        assert_eq!(nexus_source(&["nexus: 7 ".into()]).unwrap().mod_id, 7);
        assert_eq!(nexus_source(&["Nexus:abc".into(), "Nexus:-1".into()]), None);
        assert_eq!(nexus_source(&["ModDrop:5".into()]), None);
        assert_eq!(
            mod_page_url(5),
            "https://www.nexusmods.com/stardewvalley/mods/5"
        );
    }

    fn file(id: u64, name: &str, version: &str, category: NexusFileCategory) -> NexusFile {
        NexusFile {
            file_id: id,
            name: name.into(),
            version: Some(version.into()),
            category,
            file_name: format!(
                "{}-{id}-{}.zip",
                name.replace(' ', ""),
                version.replace('.', "-")
            ),
            size_bytes: 1000,
            uploaded_at: None,
        }
    }

    #[test]
    fn chooses_the_one_current_file_at_that_version() {
        use NexusFileCategory::*;
        let files = vec![
            file(1, "Lookup Anything", "1.54.0", OldVersion),
            file(2, "Lookup Anything", "1.55.0", Main),
            file(3, "Lookup Anything source", "1.55.0", Optional),
        ];
        assert_eq!(
            choose_nexus_file(&files, "1.55.0", None).unwrap().file_id,
            2
        );
        assert_eq!(choose_nexus_file(&files, "1.55", None).unwrap().file_id, 2);
        // An old version is not offered, even when asked for.
        assert_eq!(
            choose_nexus_file(&files, "1.54.0", None),
            Err(NoNexusFile::NoMatchingFile)
        );
        assert_eq!(
            choose_nexus_file(&files, "1.55.1", None),
            Err(NoNexusFile::NoMatchingFile)
        );
    }

    #[test]
    fn the_main_file_wins_and_otherwise_nothing_is_guessed() {
        use NexusFileCategory::*;
        let files = vec![
            file(1, "Content Patcher", "2.0.0", Main),
            file(2, "Content Patcher extras", "2.0.0", Optional),
        ];
        assert_eq!(choose_nexus_file(&files, "2.0.0", None).unwrap().file_id, 1);
        let two_main = vec![
            file(1, "Pack A", "2.0.0", Main),
            file(2, "Pack B", "2.0.0", Main),
        ];
        assert!(matches!(
            choose_nexus_file(&two_main, "2.0.0", None),
            Err(NoNexusFile::Ambiguous(names)) if names.len() == 2
        ));
        // A subkey picks one mod of a page that holds several.
        assert_eq!(
            choose_nexus_file(&two_main, "2.0.0", Some("Pack B"))
                .unwrap()
                .file_id,
            2
        );
    }

    #[test]
    fn only_zip_files_within_the_size_limit_are_chosen() {
        use NexusFileCategory::*;
        let mut seven = file(1, "Mod", "1.0.0", Main);
        seven.file_name = "Mod-1-0-0.7z".into();
        assert!(matches!(
            choose_nexus_file(&[seven], "1.0.0", None),
            Err(NoNexusFile::NotZip(_))
        ));
        let mut big = file(1, "Mod", "1.0.0", Main);
        big.size_bytes = MAX_RELEASE_ASSET_BYTES + 1;
        assert!(matches!(
            choose_nexus_file(&[big], "1.0.0", None),
            Err(NoNexusFile::TooLarge { .. })
        ));
        assert_eq!(NexusFileCategory::from_api("OLD_VERSION"), OldVersion);
        assert_eq!(NexusFileCategory::from_api("main"), Main);
        assert_eq!(NexusFileCategory::from_api("SOMETHING_NEW"), Other);
    }

    #[test]
    fn md5_digests_are_checked() {
        assert!(valid_md5("0123456789abcdef0123456789ABCDEF"));
        assert!(!valid_md5("0123"));
        assert!(!valid_md5("g123456789abcdef0123456789abcdef"));
    }
}
