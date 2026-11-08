//! Where a stored package was downloaded from, in plain words, for history.
//!
//! A package the manager downloaded from a GitHub release keeps the
//! repository, tag and file name with its acquisition, and the checksum
//! GitHub published when there was one (the download was refused unless it
//! matched). Activity and restore points read it back from there, so the
//! words are the same wherever the package is mentioned. A package chosen
//! from this computer has no such record and gets no line.

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

impl GitHubOrigin {
    /// "GitHub release v1.2.0 of owner/name (Mod.zip, checksum verified)".
    pub fn describe(&self) -> String {
        format!(
            "GitHub release {} of {} ({}, {})",
            self.tag,
            self.repo,
            self.asset_name,
            if self.checksum_verified {
                "checksum verified"
            } else {
                "GitHub publishes no checksum for it"
            }
        )
    }
}

fn from_acquisition(acquisition: &Acquisition) -> Option<GitHubOrigin> {
    let metadata: serde_json::Value =
        serde_json::from_str(acquisition.source_metadata.as_deref()?).ok()?;
    if metadata.get("provider").and_then(|v| v.as_str()) != Some("github") {
        return None;
    }
    let text = |key: &str| {
        metadata
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some(GitHubOrigin {
        repo: text("repo")?,
        tag: text("tag")?,
        asset_name: acquisition.original_filename.clone(),
        checksum_verified: acquisition.expected_hash.is_some(),
    })
}

/// The GitHub release a stored package was last downloaded from, if it was
/// downloaded from one. A package also chosen from this computer later
/// still came from GitHub first, so the newest GitHub record is used.
pub fn github_origin(
    package_repo: &dyn PackageCatalogRepository,
    hash: &ArtifactHash,
) -> Option<GitHubOrigin> {
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
    fn a_local_file_has_no_origin() {
        assert_eq!(from_acquisition(&acquisition(None, false)), None);
        assert_eq!(
            from_acquisition(&acquisition(Some(r#"{"provider":"nexus"}"#), false)),
            None
        );
        assert_eq!(
            from_acquisition(&acquisition(Some("not json"), false)),
            None
        );
    }
}
