//! Where a stored package was downloaded from, in plain words, for history.
//!
//! A package the manager downloaded keeps where it came from with its
//! acquisition, as JSON with a `provider`:
//!
//! - `github`: the repository, tag and file name, and the checksum GitHub
//!   published when there was one (the download was refused unless it
//!   matched).
//! - `nexus`: the mod and file ids, the file's name and version, and whether
//!   Nexus recognised the file's MD5 as that file.
//!
//! Activity and restore points read it back from here, so the words are the
//! same wherever the package is mentioned. A package chosen from this
//! computer has no such record and gets no line.

use crate::ports::repositories::PackageCatalogRepository;
use manager_core::ids::ArtifactHash;
use manager_core::package::Acquisition;

/// What a GitHub download recorded about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubOrigin {
    pub repo: String,
    pub tag: String,
    pub asset_name: String,
    /// The file matched the SHA-256 GitHub publishes for it.
    pub checksum_verified: bool,
}

/// What a Nexus Mods download recorded about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexusOrigin {
    pub mod_id: u64,
    pub file_id: u64,
    pub file_name: String,
    pub version: Option<String>,
    /// Nexus recognised the downloaded file's MD5 as this mod's file.
    pub md5_confirmed: bool,
}

/// Where a downloaded package came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageOrigin {
    GitHub(GitHubOrigin),
    Nexus(NexusOrigin),
}

impl PackageOrigin {
    /// "GitHub release v1.2.0 of owner/name (Mod.zip, checksum verified)", or
    /// "Nexus Mods file 12345 of mod 541 (Mod.zip, version 1.2.0, MD5
    /// confirmed by Nexus)".
    pub fn describe(&self) -> String {
        match self {
            Self::GitHub(o) => format!(
                "GitHub release {} of {} ({}, {})",
                o.tag,
                o.repo,
                o.asset_name,
                if o.checksum_verified {
                    "checksum verified"
                } else {
                    "GitHub publishes no checksum for it"
                }
            ),
            Self::Nexus(o) => format!(
                "Nexus Mods file {} of mod {} ({}{}, {})",
                o.file_id,
                o.mod_id,
                o.file_name,
                o.version
                    .as_deref()
                    .map(|v| format!(", version {v}"))
                    .unwrap_or_default(),
                if o.md5_confirmed {
                    "MD5 confirmed by Nexus"
                } else {
                    "Nexus did not confirm its MD5"
                }
            ),
        }
    }

    /// A few words for a label with a length limit: "GitHub owner/name
    /// v1.2.0" or "Nexus mod 541 1.2.0".
    pub fn short(&self) -> String {
        match self {
            Self::GitHub(o) => format!("GitHub {} {}", o.repo, o.tag),
            Self::Nexus(o) => match &o.version {
                Some(version) => format!("Nexus mod {} {version}", o.mod_id),
                None => format!("Nexus mod {} file {}", o.mod_id, o.file_id),
            },
        }
    }
}

fn from_acquisition(acquisition: &Acquisition) -> Option<PackageOrigin> {
    let metadata: serde_json::Value =
        serde_json::from_str(acquisition.source_metadata.as_deref()?).ok()?;
    let text = |key: &str| {
        metadata
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let id = |key: &str| {
        metadata
            .get(key)
            .and_then(|v| v.as_u64())
            .filter(|n| *n > 0)
    };
    match metadata.get("provider").and_then(|v| v.as_str())? {
        "github" => Some(PackageOrigin::GitHub(GitHubOrigin {
            repo: text("repo")?,
            tag: text("tag")?,
            asset_name: acquisition.original_filename.clone(),
            checksum_verified: acquisition.expected_hash.is_some(),
        })),
        "nexus" => Some(PackageOrigin::Nexus(NexusOrigin {
            mod_id: id("mod_id")?,
            file_id: id("file_id")?,
            file_name: acquisition.original_filename.clone(),
            version: text("version"),
            md5_confirmed: metadata
                .get("md5_confirmed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })),
        _ => None,
    }
}

/// Where a stored package was last downloaded from, if the manager
/// downloaded it. A package also chosen from this computer later still came
/// from the site first, so the newest download record is used.
pub fn package_origin(
    package_repo: &dyn PackageCatalogRepository,
    hash: &ArtifactHash,
) -> Option<PackageOrigin> {
    package_repo
        .get_acquisitions_for_artifact(hash)
        .unwrap_or_default()
        .iter()
        .find_map(from_acquisition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use manager_core::ids::AcquisitionId;
    use manager_core::package::AcquisitionSource;

    fn acquisition(metadata: Option<&str>, expected: bool) -> Acquisition {
        Acquisition {
            id: AcquisitionId::new(),
            artifact_hash: ArtifactHash::new("a".repeat(64)),
            source: AcquisitionSource::Provider,
            original_filename: "StardewUI.0.6.1.zip".into(),
            acquired_at: Utc::now(),
            expected_hash: expected.then(|| ArtifactHash::new("a".repeat(64))),
            source_metadata: metadata.map(str::to_string),
        }
    }

    #[test]
    fn reads_a_github_download_back() {
        let origin = from_acquisition(&acquisition(
            Some(r#"{"provider":"github","repo":"focustense/StardewUI","tag":"v0.6.1"}"#),
            true,
        ))
        .unwrap();
        assert_eq!(
            origin.describe(),
            "GitHub release v0.6.1 of focustense/StardewUI (StardewUI.0.6.1.zip, checksum verified)"
        );
        assert_eq!(origin.short(), "GitHub focustense/StardewUI v0.6.1");
        let unverified = from_acquisition(&acquisition(
            Some(r#"{"provider":"github","repo":"a/b","tag":"1.0"}"#),
            false,
        ))
        .unwrap();
        assert!(unverified
            .describe()
            .ends_with("GitHub publishes no checksum for it)"));
    }

    #[test]
    fn reads_a_nexus_download_back() {
        let origin = from_acquisition(&acquisition(
            Some(
                r#"{"provider":"nexus","mod_id":541,"file_id":12345,"version":"0.6.1","md5_confirmed":true}"#,
            ),
            false,
        ))
        .unwrap();
        assert_eq!(
            origin.describe(),
            "Nexus Mods file 12345 of mod 541 (StardewUI.0.6.1.zip, version 0.6.1, MD5 confirmed by Nexus)"
        );
        assert_eq!(origin.short(), "Nexus mod 541 0.6.1");
        let unconfirmed = from_acquisition(&acquisition(
            Some(r#"{"provider":"nexus","mod_id":541,"file_id":12345}"#),
            false,
        ))
        .unwrap();
        assert!(unconfirmed
            .describe()
            .ends_with("(StardewUI.0.6.1.zip, Nexus did not confirm its MD5)"));
        assert_eq!(unconfirmed.short(), "Nexus mod 541 file 12345");
    }

    #[test]
    fn a_local_file_has_no_origin() {
        assert_eq!(from_acquisition(&acquisition(None, false)), None);
        assert_eq!(
            from_acquisition(&acquisition(Some(r#"{"provider":"nexus"}"#), false)),
            None
        );
        assert_eq!(
            from_acquisition(&acquisition(Some(r#"{"provider":"moddrop"}"#), false)),
            None
        );
        assert_eq!(
            from_acquisition(&acquisition(Some("not json"), false)),
            None
        );
    }
}
