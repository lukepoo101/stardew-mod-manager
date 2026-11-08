//! Where mod updates are looked up and downloaded from.
//!
//! The update check is SMAPI's own web API, which reads Nexus, CurseForge,
//! ModDrop, GitHub and the community compatibility list for each mod. Files
//! are downloaded only from GitHub releases, which need no account. A GitHub
//! token the user adds raises GitHub's limit for reading release lists; it
//! is sent to GitHub's API only, never with a file download.

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

/// The error code a read answers with when GitHub refuses the token (401).
/// The reader does not retry; the caller decides whether to read without it.
pub const GITHUB_TOKEN_REJECTED: &str = "GITHUB_TOKEN_REJECTED";

/// GitHub's request allowance for whoever asked, from its `rate_limit` API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubRateLimit {
    /// Requests allowed per hour: 60 without a token, 5000 with one.
    pub limit: u64,
    pub remaining: u64,
    /// When the allowance is next refilled, in seconds since 1970.
    pub resets_at: u64,
}

#[async_trait]
pub trait GitHubReleasesPort: Send + Sync {
    /// The repository's most recent releases, newest first. `repo` is
    /// `owner/name` and has been checked with `valid_github_repo`. With a
    /// token, a 401 answers [`GITHUB_TOKEN_REJECTED`].
    async fn releases(&self, repo: &str, token: Option<&str>) -> AppResult<Vec<GitHubRelease>>;

    /// GitHub's allowance for requests made with `token`, or without one.
    /// Reading it does not count against the allowance.
    async fn rate_limit(&self, token: Option<&str>) -> AppResult<GitHubRateLimit>;
}

/// Where the user's GitHub token is kept: the system's keychain, never the
/// manager's own files.
pub trait GitHubTokenPort: Send + Sync {
    /// The stored token, or None when there is none.
    fn get(&self) -> AppResult<Option<String>>;
    fn set(&self, token: &str) -> AppResult<()>;
    /// Removes it; removing none is not an error.
    fn clear(&self) -> AppResult<()>;
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
