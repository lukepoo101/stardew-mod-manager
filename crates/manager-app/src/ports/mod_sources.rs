//! Where mod updates are looked up and downloaded from.
//!
//! The update check is SMAPI's own web API, which reads Nexus, CurseForge,
//! ModDrop, GitHub and the community compatibility list for each mod. Files
//! are downloaded only from GitHub releases, which need no account.

use crate::error::AppResult;
use async_trait::async_trait;
use manager_core::updates::{GitHubRelease, ModUpdateInfo, ModUpdateQuery};

#[async_trait]
pub trait ModUpdateCheckPort: Send + Sync {
    /// What is known about each mod's newest version. Every mod asked about
    /// is answered, with errors when the check could not read it.
    async fn check(
        &self,
        mods: &[ModUpdateQuery],
        smapi_version: Option<&str>,
        game_version: Option<&str>,
    ) -> AppResult<Vec<ModUpdateInfo>>;
}

#[async_trait]
pub trait GitHubReleasesPort: Send + Sync {
    /// The repository's most recent releases, newest first. `repo` is
    /// `owner/name` and has been checked with `valid_github_repo`.
    async fn releases(&self, repo: &str) -> AppResult<Vec<GitHubRelease>>;
}

#[async_trait]
pub trait ModDownloadPort: Send + Sync {
    /// Downloads a mod file into a folder of its own, checked against
    /// `expected_sha256` when given and against `expected_size` (the size
    /// the site published, 0 when unknown), and returns where it is.
    /// `file_name` is the name the site gave it; it is made safe before it
    /// is written.
    async fn download(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        expected_size: u64,
        file_name: &str,
    ) -> AppResult<std::path::PathBuf>;

    /// Deletes a file `download` returned, and its folder, once it has been
    /// stored as a package. Failing to delete it is not an error.
    fn discard(&self, downloaded: &std::path::Path);
}
