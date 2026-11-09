//! The update check SMAPI itself uses (smapi.io's web API) and GitHub's
//! release lists. Neither needs an account; a GitHub token, when the user
//! saves one, raises GitHub's limit and is sent to GitHub's API only.

use async_trait::async_trait;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::mod_sources::{
    GitHubRateLimit, GitHubReleasesPort, ModDownloadPort, ModUpdateCheckPort, SecretPort,
    GITHUB_TOKEN_REJECTED,
};
use manager_app::ports::runtime::DownloadPort;
use manager_core::updates::{
    valid_github_repo, GitHubAsset, GitHubRelease, ModUpdateInfo, ModUpdateQuery,
    MAX_RELEASE_ASSET_BYTES,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SMAPI_API_URL: &str = "https://smapi.io/api/v4.0.0/mods";
const GITHUB_API: &str = "https://api.github.com";
const GITHUB_RELEASES_PATH: &str = "/repos/{repo}/releases?per_page=30";
const GITHUB_RATE_LIMIT_PATH: &str = "/rate_limit";
/// Mods per request to smapi.io; SMAPI sends a whole Mods folder at once,
/// so this is generous.
const MODS_PER_REQUEST: usize = 200;

fn client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("StardewModManager/0.1.0")
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub struct SmapiUpdateCheck {
    client: reqwest::Client,
    url: String,
}

impl Default for SmapiUpdateCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl SmapiUpdateCheck {
    pub fn new() -> Self {
        Self::with_url(SMAPI_API_URL)
    }

    /// Against another server, for tests.
    pub fn with_url(url: &str) -> Self {
        Self {
            client: client(30),
            url: url.to_string(),
        }
    }
}

/// The request body smapi.io expects.
pub fn update_check_body(
    mods: &[ModUpdateQuery],
    smapi_version: Option<&str>,
    game_version: Option<&str>,
) -> Value {
    json!({
        "mods": mods.iter().map(|m| json!({
            "id": m.unique_id,
            "updateKeys": m.update_keys,
            "installedVersion": m.installed_version,
        })).collect::<Vec<_>>(),
        "apiVersion": smapi_version,
        "gameVersion": game_version,
        "includeExtendedMetadata": true,
    })
}

fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Reads smapi.io's answer; None when it is not the list smapi.io sends.
/// Entries without an `id` are skipped.
pub fn parse_update_check(json: &Value) -> Option<Vec<ModUpdateInfo>> {
    let list = json.as_array()?;
    let infos = list
        .iter()
        .filter_map(|entry| {
            let unique_id = text(entry, "id")?;
            let suggested = entry.get("suggestedUpdate");
            let metadata = entry.get("metadata");
            let newest = metadata.and_then(|m| m.get("main"));
            Some(ModUpdateInfo {
                unique_id,
                suggested_version: suggested.and_then(|s| text(s, "version")),
                suggested_url: suggested.and_then(|s| text(s, "url")),
                latest_version: newest.and_then(|n| text(n, "version")),
                name: metadata.and_then(|m| text(m, "name")),
                github_repo: metadata.and_then(|m| text(m, "gitHubRepo")),
                compatibility_status: metadata.and_then(|m| text(m, "compatibilityStatus")),
                compatibility_summary: metadata.and_then(|m| text(m, "compatibilitySummary")),
                errors: entry
                    .get("errors")
                    .and_then(Value::as_array)
                    .map(|errors| {
                        errors
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    Some(infos)
}

#[async_trait]
impl ModUpdateCheckPort for SmapiUpdateCheck {
    async fn check(
        &self,
        mods: &[ModUpdateQuery],
        smapi_version: Option<&str>,
        game_version: Option<&str>,
    ) -> AppResult<Vec<ModUpdateInfo>> {
        let unreachable = |e: reqwest::Error| {
            AppError::network(
                "MOD_UPDATE_CHECK_UNREACHABLE",
                if e.is_timeout() {
                    "smapi.io, which checks for updates, took too long to answer. Check your internet connection and try again."
                } else {
                    "smapi.io, which checks for updates, could not be reached. Check your internet connection and try again."
                },
            )
            .with_details(e.to_string())
        };
        let unreadable = |details: String| {
            AppError::network(
                "MOD_UPDATE_CHECK_UNREADABLE",
                "smapi.io's answer could not be read, so nothing was updated. Try again later. On a network with a sign-in page, sign in first.",
            )
            .with_details(details)
        };
        let mut out = Vec::new();
        for chunk in mods.chunks(MODS_PER_REQUEST) {
            let response = self
                .client
                .post(&self.url)
                .header("Content-Type", "application/json")
                .body(update_check_body(chunk, smapi_version, game_version).to_string())
                .send()
                .await
                .map_err(unreachable)?;
            let status = response.status();
            if !status.is_success() {
                return Err(AppError::network(
                    "MOD_UPDATE_CHECK_FAILED",
                    format!("smapi.io could not check for updates right now (it answered {status}). Try again later."),
                ));
            }
            let body = response.text().await.map_err(unreachable)?;
            let json: Value = serde_json::from_str(&body).map_err(|e| unreadable(e.to_string()))?;
            let infos = parse_update_check(&json)
                .ok_or_else(|| unreadable("the answer is not a list of mods".into()))?;
            // An answer about none of the mods asked about is not an answer.
            if infos.is_empty() && !chunk.is_empty() {
                return Err(unreadable("the answer lists no mods".into()));
            }
            out.extend(infos);
        }
        Ok(out)
    }
}

pub struct GitHubModReleases {
    client: reqwest::Client,
    /// The API's address, without a trailing slash. The token is sent only
    /// to requests on exactly this scheme, host and port.
    api: reqwest::Url,
}

impl Default for GitHubModReleases {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubModReleases {
    pub fn new() -> Self {
        Self::with_api(GITHUB_API)
    }

    /// Against another server standing in for `https://api.github.com`, for
    /// tests.
    pub fn with_api(api: &str) -> Self {
        Self {
            // reqwest drops `Authorization` when a redirect leaves the host
            // (scheme, host or port), so a redirect to a download host or
            // anywhere else never carries the token. Tests check it.
            client: client(15),
            api: reqwest::Url::parse(api.trim_end_matches('/'))
                .unwrap_or_else(|_| reqwest::Url::parse(GITHUB_API).expect("a URL")),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.api.as_str().trim_end_matches('/'))
    }

    /// A GET to GitHub's API, with the token only when `url` is the API.
    fn get(&self, url: &str, token: Option<&str>) -> reqwest::RequestBuilder {
        let request = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        match token {
            Some(token) if same_origin(url, &self.api) => {
                let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static(""));
                // Kept out of any debug output of the request.
                value.set_sensitive(true);
                request.header(reqwest::header::AUTHORIZATION, value)
            }
            _ => request,
        }
    }
}

/// Whether `url` is on the same scheme, host and port as `api`.
fn same_origin(url: &str, api: &reqwest::Url) -> bool {
    reqwest::Url::parse(url).is_ok_and(|url| {
        url.scheme() == api.scheme()
            && url.host_str() == api.host_str()
            && url.port_or_known_default() == api.port_or_known_default()
    })
}

fn token_rejected() -> AppError {
    AppError::network(
        GITHUB_TOKEN_REJECTED,
        "GitHub rejected your token (it may have expired or been revoked). Releases are read without it; replace it in Settings.",
    )
}

/// Reads GitHub's release list; None when it is not a list.
pub fn parse_github_releases(json: &Value) -> Option<Vec<GitHubRelease>> {
    let list = json.as_array()?;
    let releases = list
        .iter()
        .filter_map(|r| {
            Some(GitHubRelease {
                tag: text(r, "tag_name")?,
                name: text(r, "name"),
                html_url: text(r, "html_url").unwrap_or_default(),
                draft: r.get("draft").and_then(Value::as_bool).unwrap_or(false),
                prerelease: r
                    .get("prerelease")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                published_at: text(r, "published_at"),
                assets: r
                    .get("assets")
                    .and_then(Value::as_array)
                    .map(|assets| {
                        assets
                            .iter()
                            .filter_map(|a| {
                                let download_url = text(a, "browser_download_url")?;
                                // Only GitHub's own download addresses.
                                if !download_url.starts_with("https://github.com/") {
                                    return None;
                                }
                                Some(GitHubAsset {
                                    name: text(a, "name")?,
                                    size: a.get("size").and_then(Value::as_u64).unwrap_or(0),
                                    download_url,
                                    sha256: text(a, "digest")
                                        .and_then(|d| d.strip_prefix("sha256:").map(str::to_string))
                                        .filter(|d| {
                                            d.len() == 64
                                                && d.chars().all(|c| c.is_ascii_hexdigit())
                                        }),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    Some(releases)
}

/// How long until GitHub takes requests again, from its headers.
fn github_wait(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let number = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    if let Some(seconds) = number("retry-after") {
        return Some(seconds);
    }
    let reset = number("x-ratelimit-reset")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(reset.saturating_sub(now))
}

fn describe_wait(seconds: u64) -> String {
    match seconds {
        0..=59 => "in a minute".to_string(),
        60..=3599 => {
            let minutes = seconds.div_ceil(60);
            format!("in {minutes} minute{}", if minutes == 1 { "" } else { "s" })
        }
        _ => "in an hour".to_string(),
    }
}

#[async_trait]
impl GitHubReleasesPort for GitHubModReleases {
    async fn releases(&self, repo: &str, token: Option<&str>) -> AppResult<Vec<GitHubRelease>> {
        if !valid_github_repo(repo) {
            return Err(AppError::validation(
                "GITHUB_REPO_INVALID",
                format!("'{repo}' is not a GitHub repository name"),
            ));
        }
        let failed = |why: String| {
            AppError::network(
                "GITHUB_RELEASES_UNREACHABLE",
                format!("GitHub's releases for {repo} could not be read: {why}"),
            )
        };
        let unreachable = |e: reqwest::Error| {
            failed(if e.is_timeout() {
                "GitHub took too long to answer. Try again later.".into()
            } else {
                "GitHub could not be reached. Check your internet connection and try again.".into()
            })
            .with_details(e.to_string())
        };
        let response = self
            .get(
                &self.url(&GITHUB_RELEASES_PATH.replace("{repo}", repo)),
                token,
            )
            .send()
            .await
            .map_err(unreachable)?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED && token.is_some() {
            return Err(token_rejected());
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(failed(
                "the repository does not exist or is private.".into(),
            ));
        }
        let limited = status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || (status == reqwest::StatusCode::FORBIDDEN
                && response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| v.trim() == "0"));
        if limited {
            let when = github_wait(response.headers())
                .map(describe_wait)
                .unwrap_or_else(|| "later".into());
            return Err(failed(if token.is_some() {
                format!("GitHub's limit for your token was reached. Try again {when}.")
            } else {
                format!(
                    "GitHub's limit for requests without an account was reached. Add a GitHub token in Settings to raise it, or try again {when}."
                )
            }));
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(failed(
                "GitHub refused to list them (it answered 403 Forbidden).".into(),
            ));
        }
        if !status.is_success() {
            return Err(failed(format!(
                "GitHub answered {status}. Try again later."
            )));
        }
        let body = response.text().await.map_err(unreachable)?;
        let unreadable = |details: String| {
            failed("GitHub's answer could not be read. Try again later.".into())
                .with_details(details)
        };
        let json: Value = serde_json::from_str(&body).map_err(|e| unreadable(e.to_string()))?;
        parse_github_releases(&json).ok_or_else(|| unreadable("the answer is not a list".into()))
    }

    async fn rate_limit(&self, token: Option<&str>) -> AppResult<GitHubRateLimit> {
        let failed = |why: &str| {
            AppError::network(
                "GITHUB_RATE_LIMIT_UNREACHABLE",
                format!("GitHub's limit could not be read: {why}"),
            )
        };
        let response = self
            .get(&self.url(GITHUB_RATE_LIMIT_PATH), token)
            .send()
            .await
            .map_err(|e| {
                failed("GitHub could not be reached. Check your internet connection and try again.")
                    .with_details(e.to_string())
            })?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED && token.is_some() {
            return Err(AppError::network(
                GITHUB_TOKEN_REJECTED,
                "GitHub rejected this token. Check that it was pasted whole and has not expired or been revoked.",
            ));
        }
        if !status.is_success() {
            return Err(failed(&format!(
                "GitHub answered {status}. Try again later."
            )));
        }
        let unreadable = |details: String| {
            failed("GitHub's answer could not be read. Try again later.").with_details(details)
        };
        let body = response
            .text()
            .await
            .map_err(|e| unreadable(e.to_string()))?;
        let json: Value = serde_json::from_str(&body).map_err(|e| unreadable(e.to_string()))?;
        parse_rate_limit(&json).ok_or_else(|| {
            failed("GitHub's answer could not be read. Try again later.")
                .with_details("no rate.limit in the answer")
        })
    }
}

/// Reads GitHub's `rate_limit` answer: the core allowance, which release
/// lists count against.
pub fn parse_rate_limit(json: &Value) -> Option<GitHubRateLimit> {
    let core = json
        .get("resources")
        .and_then(|r| r.get("core"))
        .or_else(|| json.get("rate"))?;
    let number = |field: &str| core.get(field).and_then(Value::as_u64);
    Some(GitHubRateLimit {
        limit: number("limit")?,
        remaining: number("remaining")?,
        resets_at: number("reset").unwrap_or(0),
    })
}

/// One secret in the system keychain: Windows Credential Manager, the
/// macOS Keychain, or the Secret Service (GNOME Keyring, KWallet) on Linux.
pub struct KeychainSecret {
    service: String,
    account: &'static str,
    /// What it is, for messages: "GitHub token", "Nexus Mods API key".
    what: &'static str,
}

/// The GitHub token's keychain entry.
pub type KeychainGitHubToken = KeychainSecret;

const KEYCHAIN_SERVICE: &str = "StardewModManager";

impl KeychainSecret {
    const GITHUB: (&'static str, &'static str) = ("github-token", "GitHub token");
    const NEXUS: (&'static str, &'static str) = ("nexus-api-key", "Nexus Mods API key");

    /// The GitHub token.
    pub fn new() -> Self {
        Self::with_service(KEYCHAIN_SERVICE)
    }

    /// The GitHub token under another keychain service name, so tests never
    /// touch the real entry.
    pub fn with_service(service: &str) -> Self {
        Self::entry_for(service, Self::GITHUB)
    }

    /// The Nexus Mods API key.
    pub fn nexus_api_key() -> Self {
        Self::nexus_api_key_with_service(KEYCHAIN_SERVICE)
    }

    pub fn nexus_api_key_with_service(service: &str) -> Self {
        Self::entry_for(service, Self::NEXUS)
    }

    fn entry_for(service: &str, (account, what): (&'static str, &'static str)) -> Self {
        Self {
            service: service.to_string(),
            account,
            what,
        }
    }

    // AppError is the error every port returns.
    #[allow(clippy::result_large_err)]
    fn entry(&self) -> AppResult<keyring::Entry> {
        keyring::Entry::new(&self.service, self.account).map_err(|e| self.keychain_error(e))
    }

    /// The keychain's error, which never includes the secret.
    fn keychain_error(&self, error: keyring::Error) -> AppError {
        AppError::new(
            "KEYCHAIN_UNAVAILABLE",
            manager_app::error::AppErrorCategory::Runtime,
            format!(
                "The system keychain could not be used, so the {} was not read or saved. On Linux this needs a keyring such as GNOME Keyring or KWallet to be running and unlocked.",
                self.what
            ),
        )
        .with_details(error.to_string())
    }
}

impl Default for KeychainSecret {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretPort for KeychainSecret {
    fn get(&self) -> AppResult<Option<String>> {
        match self.entry()?.get_password() {
            Ok(secret) if secret.trim().is_empty() => Ok(None),
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(self.keychain_error(error)),
        }
    }

    fn set(&self, secret: &str) -> AppResult<()> {
        self.entry()?
            .set_password(secret)
            .map_err(|e| self.keychain_error(e))
    }

    fn clear(&self) -> AppResult<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(self.keychain_error(error)),
        }
    }
}

/// Mod downloads, each in a folder of its own under `dir` until it is
/// stored as a package.
pub struct ModDownloads {
    downloader: Arc<dyn DownloadPort>,
    dir: PathBuf,
}

impl ModDownloads {
    pub fn new(downloader: Arc<dyn DownloadPort>, dir: PathBuf) -> Self {
        Self { downloader, dir }
    }

    /// Deletes downloads left behind when the app stopped part way through
    /// one. Run once at startup, before anything is downloaded.
    pub fn clear_leftovers(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// A file's MD5, in lower-case hex, read in pieces.
// AppError is the error every port returns.
#[allow(clippy::result_large_err)]
pub fn file_md5(path: &Path) -> AppResult<String> {
    use md5::Digest;
    use std::io::Read;
    let failed = |e: std::io::Error| {
        AppError::system(
            "DOWNLOAD_READ_FAILED",
            "The downloaded file could not be read",
        )
        .with_details(e.to_string())
    };
    let mut file = std::fs::File::open(path).map_err(failed)?;
    let mut hasher = md5::Md5::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(failed)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(manager_core::ids::hash_to_hex(hasher.finalize()))
}

/// A site's file name made safe to write: letters, digits, `.`, `-` and `_`
/// only, never a path, always ending in `.zip`.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.to_lowercase().ends_with(".zip") && cleaned.len() > 4 {
        cleaned
    } else {
        format!(
            "{}mod.zip",
            if cleaned.is_empty() {
                String::new()
            } else {
                format!("{cleaned}-")
            }
        )
    }
}

#[async_trait]
impl ModDownloadPort for ModDownloads {
    async fn download(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        expected_size: u64,
        file_name: &str,
    ) -> AppResult<PathBuf> {
        let folder = self.dir.join(uuid::Uuid::new_v4().to_string());
        let file = folder.join(safe_file_name(file_name));
        // Never more than the file was published as, nor than the most the
        // manager downloads.
        let limit = match expected_size {
            0 => MAX_RELEASE_ASSET_BYTES,
            size => size.min(MAX_RELEASE_ASSET_BYTES),
        };
        let result = self
            .downloader
            .download_file_limited(url, expected_sha256, &file, limit)
            .await;
        let size = match &result {
            Ok(path) => std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        };
        let result = match result {
            Ok(_) if expected_size > 0 && size != expected_size => Err(AppError::validation(
                "DOWNLOAD_SIZE_MISMATCH",
                "The downloaded file is not the size it was published as, so it is not the file expected. Nothing was kept; try again.",
            )
            .with_details(format!("expected {expected_size} bytes, got {size}"))),
            other => other,
        };
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&folder);
        }
        result
    }

    fn discard(&self, downloaded: &Path) {
        // Only ever a folder this made: one level under `dir`.
        if let Some(folder) = downloaded.parent() {
            if folder.parent() == Some(self.dir.as_path()) {
                let _ = std::fs::remove_dir_all(folder);
            }
        }
    }

    fn md5(&self, downloaded: &Path) -> AppResult<String> {
        file_md5(downloaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real smapi.io answer (November 2026).
    const SMAPI_ANSWER: &str = r#"[
      {"id": "Pathoschild.LookupAnything",
       "suggestedUpdate": {"version": "1.55.0", "url": "https://www.nexusmods.com/stardewvalley/mods/541"},
       "metadata": {"id": ["Pathoschild.LookupAnything", "LookupAnything"], "name": "Lookup Anything",
         "nexusID": 541, "gitHubRepo": "Pathoschild/StardewMods",
         "main": {"version": "1.55.0", "url": "https://www.nexusmods.com/stardewvalley/mods/541"},
         "compatibilityStatus": "Ok", "compatibilitySummary": "use latest version."},
       "errors": ["Found no GitHub release for this ID."]},
      {"id": "not.a.real.mod", "metadata": {"id": []}, "errors": ["Found no GitHub repository for this ID."]},
      {"metadata": {}}
    ]"#;

    /// Trimmed from GitHub's release list for focustense/StardewUI.
    const GITHUB_ANSWER: &str = r#"[
      {"tag_name": "v0.6.1", "name": "v0.6.1", "draft": false, "prerelease": false,
       "html_url": "https://github.com/focustense/StardewUI/releases/tag/v0.6.1",
       "published_at": "2025-01-01T00:00:00Z",
       "assets": [
         {"name": "StardewUI.0.6.1.zip", "size": 844205,
          "browser_download_url": "https://github.com/focustense/StardewUI/releases/download/v0.6.1/StardewUI.0.6.1.zip",
          "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000abc"},
         {"name": "elsewhere.zip", "size": 1, "browser_download_url": "https://example.com/elsewhere.zip"},
         {"name": "bad-digest.zip", "size": 1,
          "browser_download_url": "https://github.com/a/b/releases/download/t/bad-digest.zip", "digest": "sha256:xyz"}
       ]},
      {"name": "no tag"}
    ]"#;

    #[test]
    fn smapi_answers_are_read() {
        let infos = parse_update_check(&serde_json::from_str(SMAPI_ANSWER).unwrap()).unwrap();
        assert_eq!(infos.len(), 2);
        let lookup = &infos[0];
        assert_eq!(lookup.suggested_version.as_deref(), Some("1.55.0"));
        assert_eq!(
            lookup.github_repo.as_deref(),
            Some("Pathoschild/StardewMods")
        );
        assert_eq!(lookup.compatibility_status.as_deref(), Some("Ok"));
        assert_eq!(lookup.errors.len(), 1);
        assert_eq!(infos[1].suggested_version, None);
        assert!(parse_update_check(&json!({"not": "a list"})).is_none());
    }

    #[test]
    fn the_request_carries_what_smapi_sends() {
        let body = update_check_body(
            &[ModUpdateQuery {
                unique_id: "a.b".into(),
                installed_version: "1.0.0".into(),
                update_keys: vec!["Nexus:1".into()],
            }],
            Some("4.1.10"),
            None,
        );
        assert_eq!(body["mods"][0]["id"], "a.b");
        assert_eq!(body["mods"][0]["installedVersion"], "1.0.0");
        assert_eq!(body["mods"][0]["updateKeys"][0], "Nexus:1");
        assert_eq!(body["apiVersion"], "4.1.10");
        assert!(body["gameVersion"].is_null());
        assert_eq!(body["includeExtendedMetadata"], true);
    }

    #[test]
    fn github_releases_are_read_and_foreign_links_dropped() {
        let releases =
            parse_github_releases(&serde_json::from_str(GITHUB_ANSWER).unwrap()).unwrap();
        assert_eq!(releases.len(), 1);
        let assets = &releases[0].assets;
        assert_eq!(assets.len(), 2, "the non-GitHub link is dropped");
        assert_eq!(assets[0].size, 844205);
        assert!(assets[0].sha256.as_deref().unwrap().ends_with("abc"));
        assert_eq!(
            assets[1].sha256, None,
            "a malformed digest is not a checksum"
        );
    }

    #[test]
    fn release_file_names_cannot_become_paths() {
        assert_eq!(safe_file_name("StardewUI.0.6.1.zip"), "StardewUI.0.6.1.zip");
        assert_eq!(safe_file_name("../../evil.zip"), "_.._evil.zip");
        assert_eq!(safe_file_name("a b\\c.zip"), "a_b_c.zip");
        assert_eq!(safe_file_name(""), "mod.zip");
        assert_eq!(safe_file_name(".zip"), "zip-mod.zip");
        assert_eq!(safe_file_name("noext"), "noext-mod.zip");
    }

    #[test]
    fn only_folders_it_made_are_discarded() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("downloads");
        let downloads =
            ModDownloads::new(Arc::new(crate::http::ReqwestDownloader::new()), dir.clone());
        let mine = dir.join("abc").join("Mod.zip");
        std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
        std::fs::write(&mine, b"zip").unwrap();
        let elsewhere = tmp.path().join("keep").join("Mod.zip");
        std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
        std::fs::write(&elsewhere, b"zip").unwrap();
        downloads.discard(&elsewhere);
        downloads.discard(&mine);
        assert!(elsewhere.exists());
        assert!(!mine.parent().unwrap().exists());
        assert!(dir.exists());
    }

    #[test]
    fn the_token_goes_to_the_api_only() {
        let api = reqwest::Url::parse("https://api.github.com").unwrap();
        assert!(same_origin("https://api.github.com/rate_limit", &api));
        assert!(same_origin("https://api.github.com:443/repos/a/b", &api));
        assert!(!same_origin("http://api.github.com/rate_limit", &api));
        assert!(!same_origin(
            "https://objects.githubusercontent.com/x",
            &api
        ));
        assert!(!same_origin(
            "https://github.com/a/b/releases/download/t/f.zip",
            &api
        ));
        assert!(!same_origin("https://api.github.com.evil.example/", &api));
        let github = GitHubModReleases::new();
        let with = github
            .get("https://api.github.com/rate_limit", Some("abc"))
            .build()
            .unwrap();
        assert_eq!(with.headers()["authorization"], "Bearer abc");
        assert!(format!("{with:?}").find("abc").is_none(), "{with:?}");
        let elsewhere = github
            .get("https://objects.githubusercontent.com/x", Some("abc"))
            .build()
            .unwrap();
        assert!(elsewhere.headers().get("authorization").is_none());
    }

    #[test]
    fn rate_limits_are_read() {
        let answer = json!({"resources": {"core": {"limit": 5000, "remaining": 4999, "reset": 1700000000, "used": 1}},
                            "rate": {"limit": 5000, "remaining": 4999, "reset": 1700000000}});
        let limit = parse_rate_limit(&answer).unwrap();
        assert_eq!(
            (limit.limit, limit.remaining, limit.resets_at),
            (5000, 4999, 1700000000)
        );
        assert!(parse_rate_limit(&json!({})).is_none());
    }

    #[tokio::test]
    async fn repository_names_are_checked_before_any_request() {
        let github = GitHubModReleases::with_api("http://127.0.0.1:9");
        let error = github.releases("../../user", Some("t")).await.unwrap_err();
        assert_eq!(error.code, "GITHUB_REPO_INVALID");
    }
}
