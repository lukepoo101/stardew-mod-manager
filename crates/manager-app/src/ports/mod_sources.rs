//! Where mod updates are looked up and downloaded from.
//!
//! The update check is SMAPI's own web API, which reads Nexus, CurseForge,
//! ModDrop, GitHub and the community compatibility list for each mod. Files
//! are downloaded from GitHub releases, which need no account, and from
//! Nexus Mods when the user has connected it. A GitHub token the user adds
//! raises GitHub's limit for reading release lists; it is sent to GitHub's
//! API only, never with a file download. A Nexus API key is sent to Nexus's
//! API only, never with a file download either.

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

/// Where one secret the user gave the manager is kept (a GitHub token, a
/// Nexus API key): the system's keychain, never the manager's own files.
pub trait SecretPort: Send + Sync {
    /// The stored secret, or None when there is none.
    fn get(&self) -> AppResult<Option<String>>;
    fn set(&self, secret: &str) -> AppResult<()>;
    /// Removes it; removing none is not an error.
    fn clear(&self) -> AppResult<()>;
}

/// Where the user's GitHub token is kept.
pub use self::SecretPort as GitHubTokenPort;

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

    /// The MD5 of a file `download` returned, in lower-case hex. Nexus
    /// identifies its files by MD5, so a download from it is checked this
    /// way.
    fn md5(&self, downloaded: &std::path::Path) -> AppResult<String>;
}

/// The error code a Nexus read answers with when Nexus refuses the API key
/// (401). The caller says so and stops using it until it is replaced.
pub const NEXUS_KEY_REJECTED: &str = "NEXUS_KEY_REJECTED";

/// The error code a download link request answers with when Nexus says the
/// account needs Premium to download through the API without a link from
/// the site's "Mod manager download" button (403).
pub const NEXUS_PREMIUM_REQUIRED: &str = "NEXUS_PREMIUM_REQUIRED";

/// The error code any Nexus read answers with when its rate limit for the
/// account is used up (429).
pub const NEXUS_RATE_LIMITED: &str = "NEXUS_RATE_LIMITED";

/// The account a Nexus API key belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexusAccount {
    pub user_id: u64,
    pub name: String,
    /// Premium members can download through the API without a link from the
    /// site.
    pub is_premium: bool,
    /// What is left of Nexus's request allowance, when it said.
    pub hourly_remaining: Option<u64>,
    pub daily_remaining: Option<u64>,
}

/// The one-file permission an `nxm://` link carries.
#[derive(Clone, PartialEq, Eq)]
pub struct NexusLinkKey {
    pub key: String,
    pub expires: u64,
}

impl std::fmt::Debug for NexusLinkKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NexusLinkKey")
            .field("key", &"<redacted>")
            .field("expires", &self.expires)
            .finish()
    }
}

/// A file Nexus recognised by its MD5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexusMd5Match {
    pub mod_id: u64,
    pub file_id: u64,
}

/// Nexus Mods' API (v1), for Stardew Valley. Every call is made because the
/// user asked for something, and carries the user's own API key, which is
/// sent to the API's own address only.
#[async_trait]
pub trait NexusApiPort: Send + Sync {
    /// Whose key it is, and whether they have Premium. A 401 answers
    /// [`NEXUS_KEY_REJECTED`].
    async fn account(&self, api_key: &str) -> AppResult<NexusAccount>;

    /// A mod's files, as its Files tab lists them.
    async fn mod_files(
        &self,
        api_key: &str,
        mod_id: u64,
    ) -> AppResult<Vec<manager_core::nexus::NexusFile>>;

    /// Where one file can be downloaded from now: a short-lived address on
    /// Nexus's download servers. Without `link_key`, Nexus allows this for
    /// Premium members only and otherwise answers [`NEXUS_PREMIUM_REQUIRED`].
    async fn download_url(
        &self,
        api_key: &str,
        mod_id: u64,
        file_id: u64,
        link_key: Option<&NexusLinkKey>,
    ) -> AppResult<String>;

    /// The files of Stardew Valley mods whose MD5 is `md5`. None found is an
    /// empty list, not an error.
    async fn find_by_md5(&self, api_key: &str, md5: &str) -> AppResult<Vec<NexusMd5Match>>;
}
