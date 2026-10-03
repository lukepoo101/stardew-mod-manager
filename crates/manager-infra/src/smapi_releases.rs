//! SMAPI releases as SMAPI publishes them: the GitHub releases of
//! Pathoschild/SMAPI for versions and installers (with the checksum GitHub
//! records for each file, where it has one), and SMAPI's own Constants.cs at
//! each release tag for the game versions it supports. No account is used.

use async_trait::async_trait;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::smapi_releases::SmapiReleaseSourcePort;
use manager_core::smapi::catalog::{parse_constants, SmapiRelease};

const RELEASES_URL: &str = "https://api.github.com/repos/Pathoschild/SMAPI/releases?per_page=100";
const CONSTANTS_URL: &str =
    "https://raw.githubusercontent.com/Pathoschild/SMAPI/{tag}/src/SMAPI/Constants.cs";

pub struct GitHubSmapiReleases {
    client: reqwest::Client,
}

impl Default for GitHubSmapiReleases {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubSmapiReleases {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("StardewModManager/0.1.0")
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

/// Reads GitHub's release list into releases with a player installer.
pub fn parse_release_list(json: &serde_json::Value) -> Vec<SmapiRelease> {
    let Some(list) = json.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter(|r| !r.get("draft").and_then(|d| d.as_bool()).unwrap_or(false))
        .filter_map(|r| {
            let tag = r.get("tag_name")?.as_str()?.to_string();
            // The player installer: "SMAPI-<version>-installer.zip", not the
            // developer or double-zipped variants.
            let asset = r.get("assets")?.as_array()?.iter().find(|a| {
                a.get("name").and_then(|n| n.as_str()).is_some_and(|n| {
                    n.starts_with("SMAPI-")
                        && n.ends_with("-installer.zip")
                        && !n.contains("for-developers")
                        && !n.contains("double-zipped")
                })
            })?;
            let name = asset.get("name")?.as_str()?;
            let version = name
                .strip_prefix("SMAPI-")?
                .strip_suffix("-installer.zip")?
                .to_string();
            Some(SmapiRelease {
                version,
                tag: tag.clone(),
                published_at: r
                    .get("published_at")
                    .and_then(|p| p.as_str())
                    .map(str::to_string),
                prerelease: r
                    .get("prerelease")
                    .and_then(|p| p.as_bool())
                    .unwrap_or(false),
                installer_url: asset.get("browser_download_url")?.as_str()?.to_string(),
                sha256: asset
                    .get("digest")
                    .and_then(|d| d.as_str())
                    .and_then(|d| d.strip_prefix("sha256:"))
                    .map(str::to_string),
                min_game: None,
                max_game: None,
                notes_url: r
                    .get("html_url")
                    .and_then(|u| u.as_str())
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect()
}

#[async_trait]
impl SmapiReleaseSourcePort for GitHubSmapiReleases {
    async fn list_releases(&self) -> AppResult<Vec<SmapiRelease>> {
        let offline = |e: String| {
            AppError::network(
                "SMAPI_RELEASES_UNREACHABLE",
                format!("SMAPI's release list could not be read: {e}"),
            )
        };
        let response = self
            .client
            .get(RELEASES_URL)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| offline(e.to_string()))?;
        if !response.status().is_success() {
            return Err(offline(format!("GitHub answered {}", response.status())));
        }
        let text = response.text().await.map_err(|e| offline(e.to_string()))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| offline(e.to_string()))?;
        let releases = parse_release_list(&json);
        if releases.is_empty() {
            return Err(offline("the list had no installers".to_string()));
        }
        Ok(releases)
    }

    async fn game_range(&self, tag: &str) -> AppResult<(Option<String>, Option<String>)> {
        let plain = !tag.is_empty()
            && tag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !plain {
            return Err(AppError::validation(
                "SMAPI_TAG_INVALID",
                "Not a release tag",
            ));
        }
        let url = CONSTANTS_URL.replace("{tag}", tag);
        let failed = |e: String| AppError::network("SMAPI_RANGE_UNREACHABLE", e);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| failed(e.to_string()))?;
        if !response.status().is_success() {
            return Err(failed(format!("{url} answered {}", response.status())));
        }
        let text = response.text().await.map_err(|e| failed(e.to_string()))?;
        Ok(parse_constants(&text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_player_installer_and_its_published_checksum_are_picked() {
        let json = serde_json::json!([
            {
                "tag_name": "4.5.2",
                "prerelease": false,
                "draft": false,
                "published_at": "2026-01-01T00:00:00Z",
                "html_url": "https://github.com/Pathoschild/SMAPI/releases/tag/4.5.2",
                "assets": [
                    {"name": "SMAPI-4.5.2-installer-double-zipped.zip", "browser_download_url": "x", "digest": "sha256:11"},
                    {"name": "SMAPI-4.5.2-installer.zip", "browser_download_url": "https://dl/4.5.2.zip", "digest": "sha256:dd01"}
                ]
            },
            {
                "tag_name": "3.1",
                "prerelease": false,
                "assets": [
                    {"name": "SMAPI-3.1.0-installer-for-developers.zip", "browser_download_url": "dev"},
                    {"name": "SMAPI-3.1.0-installer.zip", "browser_download_url": "https://dl/3.1.0.zip", "digest": null}
                ]
            },
            {"tag_name": "draft", "draft": true, "assets": []},
            {"tag_name": "1.0", "assets": [{"name": "readme.txt", "browser_download_url": "z"}]}
        ]);
        let releases = parse_release_list(&json);
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].version, "4.5.2");
        assert_eq!(releases[0].installer_url, "https://dl/4.5.2.zip");
        assert_eq!(releases[0].sha256.as_deref(), Some("dd01"));
        assert_eq!(releases[1].version, "3.1.0");
        assert_eq!(releases[1].tag, "3.1");
        assert_eq!(releases[1].sha256, None);
    }
}
