//! Nexus Mods' public API (v1), for Stardew Valley.
//!
//! Every request identifies the manager with the `Application-Name` and
//! `Application-Version` headers Nexus's API Acceptable Use Policy asks
//! for, and carries the user's key in the `apikey` header, marked sensitive
//! so it never appears in debug output. The key is sent only to the API's
//! own scheme, host and port: the download addresses Nexus hands back carry
//! their own permission and are fetched by the ordinary downloader, which
//! never sees the key.
//!
//! Nothing here keeps or logs an answer's body: `users/validate` answers
//! with the key and the account's email address, and only the user id,
//! name and Premium status are read from it.

// AppError is the error every port returns.
#![allow(clippy::result_large_err)]

use async_trait::async_trait;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::mod_sources::{
    NexusAccount, NexusApiPort, NexusLinkKey, NexusMd5Match, NEXUS_KEY_REJECTED,
    NEXUS_PREMIUM_REQUIRED, NEXUS_RATE_LIMITED,
};
use manager_core::nexus::{valid_md5, NexusFile, NexusFileCategory, STARDEW_GAME_DOMAIN};
use serde_json::Value;

const NEXUS_API: &str = "https://api.nexusmods.com";

/// The name Nexus knows the manager by. It stays the same across versions,
/// as the policy asks.
pub const APPLICATION_NAME: &str = "StardewModManager";

/// The manager's version, sent with every request.
pub const APPLICATION_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct NexusApi {
    client: reqwest::Client,
    api: reqwest::Url,
}

impl Default for NexusApi {
    fn default() -> Self {
        Self::new()
    }
}

impl NexusApi {
    pub fn new() -> Self {
        Self::with_api(NEXUS_API)
    }

    /// Against another server standing in for `https://api.nexusmods.com`,
    /// for tests.
    pub fn with_api(api: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(format!("{APPLICATION_NAME}/{APPLICATION_VERSION}"))
                .timeout(std::time::Duration::from_secs(15))
                // A redirect is never followed: the API does not redirect,
                // and following one could carry the key elsewhere.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            api: reqwest::Url::parse(api.trim_end_matches('/'))
                .unwrap_or_else(|_| reqwest::Url::parse(NEXUS_API).expect("a URL")),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.api.as_str().trim_end_matches('/'))
    }

    /// A GET to the API, with the key.
    async fn get(
        &self,
        path: &str,
        api_key: &str,
    ) -> AppResult<(reqwest::StatusCode, reqwest::header::HeaderMap, String)> {
        let mut key = reqwest::header::HeaderValue::from_str(api_key).map_err(|_| {
            AppError::validation("NEXUS_KEY_INVALID", "That is not a Nexus Mods API key")
        })?;
        key.set_sensitive(true);
        let response = self
            .client
            .get(self.url(path))
            .header("apikey", key)
            .header("Application-Name", APPLICATION_NAME)
            .header("Application-Version", APPLICATION_VERSION)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| {
                AppError::network(
                    "NEXUS_UNREACHABLE",
                    if e.is_timeout() {
                        "Nexus Mods took too long to answer. Try again later."
                    } else {
                        "Nexus Mods could not be reached. Check your internet connection and try again."
                    },
                )
                // reqwest's error names the address, which holds no secret
                // (the key is a header).
                .with_details(e.to_string())
            })?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.text().await.map_err(|e| {
            AppError::network(
                "NEXUS_UNREACHABLE",
                "Nexus Mods' answer could not be read. Try again later.",
            )
            .with_details(e.to_string())
        })?;
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AppError::network(
                NEXUS_KEY_REJECTED,
                "Nexus Mods refused the API key.",
            ));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::network(
                NEXUS_RATE_LIMITED,
                format!(
                    "Nexus Mods' limit on requests for your account was reached. Try again {}.",
                    rate_reset(&headers).unwrap_or_else(|| "later".into())
                ),
            ));
        }
        Ok((status, headers, body))
    }
}

/// When Nexus's hourly allowance refills, in words, from its headers.
fn rate_reset(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let reset = headers
        .get("x-rl-hourly-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| chrono::DateTime::parse_from_rfc3339(v.trim()).ok())?;
    let minutes = (reset.with_timezone(&chrono::Utc) - chrono::Utc::now())
        .num_minutes()
        .max(1);
    Some(if minutes >= 60 {
        "in an hour".into()
    } else {
        format!("in {minutes} minute{}", if minutes == 1 { "" } else { "s" })
    })
}

fn header_number(headers: &reqwest::header::HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
}

fn unreadable(what: &str, details: impl Into<String>) -> AppError {
    AppError::network(
        "NEXUS_ANSWER_UNREADABLE",
        format!("Nexus Mods' answer about {what} could not be read. Try again later."),
    )
    .with_details(details)
}

fn json(body: &str, what: &str) -> AppResult<Value> {
    // The body is not put in the error: it may hold the key or an email.
    serde_json::from_str(body).map_err(|e| unreadable(what, e.to_string()))
}

fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Reads `users/validate`: only the id, name and Premium status.
pub fn parse_account(json: &Value) -> Option<(u64, String, bool)> {
    let user_id = json.get("user_id").and_then(Value::as_u64)?;
    let name = text(json, "name")?;
    let is_premium = json
        .get("is_premium")
        .and_then(Value::as_bool)
        .or_else(|| json.get("is_premium?").and_then(Value::as_bool))
        .unwrap_or(false);
    Some((user_id, name, is_premium))
}

/// Reads a mod's `files` list. Files without an id or file name are left
/// out.
pub fn parse_files(json: &Value) -> Option<Vec<NexusFile>> {
    let files = json.get("files")?.as_array()?;
    Some(
        files
            .iter()
            .filter_map(|f| {
                Some(NexusFile {
                    file_id: f.get("file_id").and_then(Value::as_u64)?,
                    name: text(f, "name").unwrap_or_default(),
                    version: text(f, "version").or_else(|| text(f, "mod_version")),
                    category: text(f, "category_name")
                        .map(|c| NexusFileCategory::from_api(&c))
                        .unwrap_or(NexusFileCategory::Other),
                    file_name: text(f, "file_name")?,
                    size_bytes: f.get("size_in_bytes").and_then(Value::as_u64).unwrap_or(0),
                    uploaded_at: f.get("uploaded_timestamp").and_then(Value::as_i64),
                })
            })
            .collect(),
    )
}

/// Whether a download address is one of Nexus's own, over https.
pub fn nexus_download_host(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.host_str().is_some_and(|host| {
                let host = host.to_ascii_lowercase();
                ["nexusmods.com", "nexus-cdn.com"]
                    .iter()
                    .any(|site| host == *site || host.ends_with(&format!(".{site}")))
            })
    })
}

/// Reads `download_link`: the first of Nexus's own https addresses.
pub fn parse_download_link(json: &Value) -> Option<String> {
    json.as_array()?
        .iter()
        .filter_map(|l| text(l, "URI"))
        .find(|uri| nexus_download_host(uri))
}

/// Reads `md5_search`.
pub fn parse_md5_matches(json: &Value) -> Option<Vec<NexusMd5Match>> {
    Some(
        json.as_array()?
            .iter()
            .filter_map(|m| {
                Some(NexusMd5Match {
                    mod_id: m.get("mod")?.get("mod_id")?.as_u64()?,
                    file_id: m.get("file_details")?.get("file_id")?.as_u64()?,
                })
            })
            .collect(),
    )
}

#[async_trait]
impl NexusApiPort for NexusApi {
    async fn account(&self, api_key: &str) -> AppResult<NexusAccount> {
        let (status, headers, body) = self.get("/v1/users/validate.json", api_key).await?;
        if !status.is_success() {
            return Err(AppError::network(
                "NEXUS_ACCOUNT_UNREADABLE",
                format!("Nexus Mods answered {status} when asked about the key. Try again later."),
            ));
        }
        let (user_id, name, is_premium) = parse_account(&json(&body, "your account")?)
            .ok_or_else(|| unreadable("your account", "no user_id or name"))?;
        Ok(NexusAccount {
            user_id,
            name,
            is_premium,
            hourly_remaining: header_number(&headers, "x-rl-hourly-remaining"),
            daily_remaining: header_number(&headers, "x-rl-daily-remaining"),
        })
    }

    async fn mod_files(&self, api_key: &str, mod_id: u64) -> AppResult<Vec<NexusFile>> {
        let (status, _, body) = self
            .get(
                &format!("/v1/games/{STARDEW_GAME_DOMAIN}/mods/{mod_id}/files.json"),
                api_key,
            )
            .await?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::validation(
                "NEXUS_MOD_NOT_FOUND",
                format!("Nexus Mods has no Stardew Valley mod {mod_id}, or it is hidden."),
            ));
        }
        if !status.is_success() {
            return Err(AppError::network(
                "NEXUS_FILES_UNREADABLE",
                format!("Nexus Mods answered {status} when asked for mod {mod_id}'s files. Try again later."),
            ));
        }
        parse_files(&json(&body, "the mod's files")?)
            .ok_or_else(|| unreadable("the mod's files", "no files list"))
    }

    async fn download_url(
        &self,
        api_key: &str,
        mod_id: u64,
        file_id: u64,
        link_key: Option<&NexusLinkKey>,
    ) -> AppResult<String> {
        let mut path = format!(
            "/v1/games/{STARDEW_GAME_DOMAIN}/mods/{mod_id}/files/{file_id}/download_link.json"
        );
        if let Some(link) = link_key {
            // Checked when the link was read: letters, digits, '-' and '_'.
            if !link
                .key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(AppError::validation(
                    "NXM_LINK_MALFORMED",
                    "That Nexus Mods link's key is not valid.",
                ));
            }
            path.push_str(&format!("?key={}&expires={}", link.key, link.expires));
        }
        let (status, _, body) = self.get(&path, api_key).await?;
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::network(
                NEXUS_PREMIUM_REQUIRED,
                "Nexus Mods allows direct downloads through other apps for Premium members only.",
            ));
        }
        if status == reqwest::StatusCode::GONE {
            return Err(AppError::validation(
                "NXM_LINK_EXPIRED",
                "That Nexus Mods download link has expired. Use \"Mod manager download\" on the mod's page again.",
            ));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::validation(
                "NEXUS_FILE_NOT_FOUND",
                "Nexus Mods no longer has that file. It may have been removed.",
            ));
        }
        if !status.is_success() {
            return Err(AppError::network(
                "NEXUS_DOWNLOAD_LINK_UNREADABLE",
                format!("Nexus Mods answered {status} when asked for the file. Try again later."),
            ));
        }
        parse_download_link(&json(&body, "the download")?)
            .ok_or_else(|| unreadable("the download", "no https address on Nexus's own servers"))
    }

    async fn find_by_md5(&self, api_key: &str, md5: &str) -> AppResult<Vec<NexusMd5Match>> {
        if !valid_md5(md5) {
            return Err(AppError::validation(
                "NEXUS_MD5_INVALID",
                "Not an MD5 digest",
            ));
        }
        let (status, _, body) = self
            .get(
                &format!(
                    "/v1/games/{STARDEW_GAME_DOMAIN}/mods/md5_search/{}.json",
                    md5.to_ascii_lowercase()
                ),
                api_key,
            )
            .await?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if !status.is_success() {
            return Err(AppError::network(
                "NEXUS_MD5_UNREADABLE",
                format!("Nexus Mods answered {status} when asked about the file. Try again later."),
            ));
        }
        parse_md5_matches(&json(&body, "the file")?)
            .ok_or_else(|| unreadable("the file", "not a list"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Write};
    use std::sync::{Arc, Mutex};

    /// A request the stand-in received: its path and headers.
    #[derive(Debug, Clone)]
    struct Seen {
        path: String,
        headers: Vec<(String, String)>,
    }

    impl Seen {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }
    }

    type Answer = dyn Fn(&str) -> (u16, Vec<(String, String)>, String) + Send + Sync;

    /// A local server standing in for api.nexusmods.com.
    fn serve(
        answer: impl Fn(&str) -> (u16, Vec<(String, String)>, String) + Send + Sync + 'static,
    ) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let answer: Arc<Answer> = Arc::new(answer);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let path = request_line.split(' ').nth(1).unwrap_or("").to_string();
                let mut headers = Vec::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        headers.push((name.trim().to_string(), value.trim().to_string()));
                    }
                }
                let (status, extra, body) = answer(&path);
                log.lock().unwrap().push(Seen { path, headers });
                let mut response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (name, value) in extra {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str("\r\n");
                response.push_str(&body);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (base, seen)
    }

    const KEY: &str = "TESTKEY+abc/123==--xyz";

    fn ok(body: Value) -> (u16, Vec<(String, String)>, String) {
        (200, Vec::new(), body.to_string())
    }

    #[tokio::test]
    async fn every_request_names_the_manager_and_carries_the_key() {
        let (base, seen) = serve(|_| {
            (
                200,
                vec![
                    ("x-rl-hourly-remaining".into(), "99".into()),
                    ("x-rl-daily-remaining".into(), "19999".into()),
                ],
                json!({"user_id": 7, "key": KEY, "name": "Farmer", "is_premium": true,
                       "email": "farmer@example.com"})
                .to_string(),
            )
        });
        let account = NexusApi::with_api(&base).account(KEY).await.unwrap();
        assert_eq!(
            account,
            NexusAccount {
                user_id: 7,
                name: "Farmer".into(),
                is_premium: true,
                hourly_remaining: Some(99),
                daily_remaining: Some(19999),
            }
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].path, "/v1/users/validate.json");
        assert_eq!(seen[0].header("apikey"), Some(KEY));
        assert_eq!(seen[0].header("Application-Name"), Some(APPLICATION_NAME));
        assert_eq!(
            seen[0].header("Application-Version"),
            Some(APPLICATION_VERSION)
        );
    }

    #[tokio::test]
    async fn a_refused_key_and_the_rate_limit_are_told_apart() {
        let (base, _) = serve(|path| {
            if path.contains("validate") {
                (
                    401,
                    Vec::new(),
                    json!({"message": "Please provide a valid API Key"}).to_string(),
                )
            } else {
                (429, Vec::new(), String::new())
            }
        });
        let api = NexusApi::with_api(&base);
        let refused = api.account(KEY).await.unwrap_err();
        assert_eq!(refused.code, NEXUS_KEY_REJECTED);
        let limited = api.mod_files(KEY, 1).await.unwrap_err();
        assert_eq!(limited.code, NEXUS_RATE_LIMITED);
    }

    #[tokio::test]
    async fn reads_files_download_links_and_md5_matches() {
        let (base, seen) = serve(|path| {
            if path.ends_with("/mods/541/files.json") {
                ok(json!({"files": [
                    {"file_id": 10, "name": "Lookup Anything", "version": "1.55.0",
                     "category_name": "MAIN", "file_name": "Lookup Anything-541-1-55-0.zip",
                     "size_in_bytes": 1234, "uploaded_timestamp": 1_700_000_000},
                    {"file_id": 9, "name": "Old", "version": "1.54.0", "category_name": "OLD_VERSION",
                     "file_name": "old.zip"},
                    {"name": "no id"}
                ], "file_updates": []}))
            } else if path.contains("/download_link.json") {
                ok(json!([
                    {"name": "Elsewhere", "short_name": "x", "URI": "https://example.com/f.zip"},
                    {"name": "Nexus CDN", "short_name": "Nexus CDN",
                     "URI": "https://cf-files.nexusmods.com/cdn/1303/541/f.zip?md5=abc&expires=1"}
                ]))
            } else if path.contains("md5_search/0123456789abcdef0123456789abcdef") {
                ok(json!([{"mod": {"mod_id": 541}, "file_details": {"file_id": 10, "md5": "x"}}]))
            } else {
                (
                    404,
                    Vec::new(),
                    json!({"message": "No file found"}).to_string(),
                )
            }
        });
        let api = NexusApi::with_api(&base);
        let files = api.mod_files(KEY, 541).await.unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].category, NexusFileCategory::Main);
        assert_eq!(files[0].size_bytes, 1234);
        assert_eq!(files[1].size_bytes, 0);

        let url = api
            .download_url(
                KEY,
                541,
                10,
                Some(&NexusLinkKey {
                    key: "abc-DEF_1".into(),
                    expires: 1_800_000_000,
                }),
            )
            .await
            .unwrap();
        assert!(url.starts_with("https://cf-files.nexusmods.com/"), "{url}");
        assert!(seen.lock().unwrap().iter().any(|s| s.path
            == "/v1/games/stardewvalley/mods/541/files/10/download_link.json?key=abc-DEF_1&expires=1800000000"));

        assert_eq!(
            api.find_by_md5(KEY, "0123456789ABCDEF0123456789abcdef")
                .await
                .unwrap(),
            vec![NexusMd5Match {
                mod_id: 541,
                file_id: 10
            }]
        );
        assert!(api
            .find_by_md5(KEY, "ffffffffffffffffffffffffffffffff")
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            api.find_by_md5(KEY, "../../etc").await.unwrap_err().code,
            "NEXUS_MD5_INVALID"
        );
        assert_eq!(
            api.mod_files(KEY, 999).await.unwrap_err().code,
            "NEXUS_MOD_NOT_FOUND"
        );
    }

    #[tokio::test]
    async fn without_premium_a_direct_download_is_refused() {
        let (base, _) = serve(|_| {
            (
                403,
                Vec::new(),
                json!({"message": "premium only"}).to_string(),
            )
        });
        let error = NexusApi::with_api(&base)
            .download_url(KEY, 1, 2, None)
            .await
            .unwrap_err();
        assert_eq!(error.code, NEXUS_PREMIUM_REQUIRED);
    }

    #[tokio::test]
    async fn a_redirect_is_not_followed() {
        let (elsewhere, seen_elsewhere) = serve(|_| ok(json!({"user_id": 1, "name": "x"})));
        let target = format!("{elsewhere}/steal");
        let (base, _) = serve(move |_| {
            (
                302,
                vec![("Location".into(), target.clone())],
                String::new(),
            )
        });
        let result = NexusApi::with_api(&base).account(KEY).await;
        assert!(result.is_err());
        assert!(seen_elsewhere.lock().unwrap().is_empty());
    }

    #[test]
    fn only_nexus_download_hosts_are_accepted() {
        for good in [
            "https://cf-files.nexusmods.com/a.zip",
            "https://supporter-files.nexus-cdn.com/a.zip?x=1",
            "https://nexusmods.com/a.zip",
        ] {
            assert!(nexus_download_host(good), "{good}");
        }
        for bad in [
            "http://cf-files.nexusmods.com/a.zip",
            "https://evilnexusmods.com/a.zip",
            "https://nexusmods.com.evil.com/a.zip",
            "https://user:pw@cf-files.nexusmods.com/a.zip",
            "not a url",
        ] {
            assert!(!nexus_download_host(bad), "{bad}");
        }
    }

    #[test]
    fn only_what_is_needed_is_read_from_the_account() {
        let account = parse_account(&json!({
            "user_id": 3, "name": "A", "is_premium?": false, "key": "secret", "email": "a@b"
        }))
        .unwrap();
        assert_eq!(account, (3, "A".to_string(), false));
        assert!(parse_account(&json!({"name": "no id"})).is_none());
    }
}
