//! The update check SMAPI itself uses (smapi.io's web API) and GitHub's
//! release lists. Neither needs an account.

use async_trait::async_trait;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::mod_sources::{GitHubReleasesPort, ModDownloadPort, ModUpdateCheckPort};
use manager_app::ports::runtime::DownloadPort;
use manager_core::updates::{
    valid_github_repo, GitHubAsset, GitHubRelease, ModUpdateInfo, ModUpdateQuery,
    MAX_RELEASE_ASSET_BYTES,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SMAPI_API_URL: &str = "https://smapi.io/api/v4.0.0/mods";
const GITHUB_RELEASES_URL: &str = "https://api.github.com/repos/{repo}/releases?per_page=30";
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
    url: String,
}

impl Default for GitHubModReleases {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubModReleases {
    pub fn new() -> Self {
        Self::with_url(GITHUB_RELEASES_URL)
    }

    /// Against another server, for tests; `{repo}` is replaced.
    pub fn with_url(url: &str) -> Self {
        Self {
            client: client(15),
            url: url.to_string(),
        }
    }
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
    async fn releases(&self, repo: &str) -> AppResult<Vec<GitHubRelease>> {
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
            .client
            .get(self.url.replace("{repo}", repo))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(unreachable)?;
        let status = response.status();
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
            return Err(failed(format!(
                "GitHub's limit for requests without an account was reached. Try again {when}."
            )));
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

    #[tokio::test]
    async fn repository_names_are_checked_before_any_request() {
        let github = GitHubModReleases::with_url("http://127.0.0.1:9/{repo}");
        let error = github.releases("../../user").await.unwrap_err();
        assert_eq!(error.code, "GITHUB_REPO_INVALID");
    }
}
