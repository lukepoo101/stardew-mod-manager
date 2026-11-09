//! Downloading mods from Nexus Mods into the same install review a chosen
//! archive gets.
//!
//! Two ways in, both started by the user:
//!
//! - **Download update** on a mod the update check found an update for,
//!   whose manifest names its Nexus page (`Nexus:1234`). Nexus lets only
//!   Premium members download through the API this way.
//! - **An `nxm://` link** from the site's "Mod manager download" button,
//!   which carries a short-lived key for one file, so it works without
//!   Premium. The link waits here, its key kept out of the window, until the
//!   user says which profile it is for.
//!
//! A file is chosen as strictly as a GitHub release file (see
//! `manager_core::nexus::choose_nexus_file`), downloaded without the API key
//! (Nexus's download addresses carry their own permission), checked against
//! the size Nexus lists, and its MD5 is looked up on Nexus to confirm it is
//! the file asked for. It is then inspected like any archive, and an update
//! is discarded unless the archive holds the mod at the version it was
//! downloaded as.
//!
//! **Until the manager is registered with Nexus, only development builds
//! can use this.** Nexus's API Acceptable Use Policy allows a personal API
//! key in an app being tested or for personal use, and not in an app for a
//! wider audience. A build without the `nexus-personal-key` feature says
//! Nexus is not connected and never reads, saves or sends a key. Signing in
//! through Nexus (stage 2, after registration) replaces the personal key.

use crate::api::dto::{
    DownloadSourceDto, DownloadedPreviewDto, ModListItemDto, NexusAccountDto, NexusDownloadDto,
    NexusStatusDto, NxmInboxDto, NxmLinkDto,
};
use crate::error::{AppError, AppResult};
use crate::ports::mod_sources::{
    ModDownloadPort, NexusAccount, NexusApiPort, NexusLinkKey, SecretPort, NEXUS_KEY_REJECTED,
    NEXUS_PREMIUM_REQUIRED,
};
use crate::services::mod_updates::{same_version, ModUpdatesService};
use crate::services::mods::ModsService;
use crate::services::operations::OperationsService;
use crate::services::packages::DownloadOrigin;
use manager_core::ids::{OperationId, ProfileId};
use manager_core::nexus::{
    choose_nexus_file, mod_page_url, nexus_source, NexusFile, NxmLink, STARDEW_GAME_DOMAIN,
};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The longest API key accepted. Nexus's personal keys are well under this.
const MAX_KEY_LEN: usize = 512;

/// How many received links are kept waiting at once. Clicking the button
/// over and over does not grow this without end.
const MAX_WAITING_LINKS: usize = 20;

/// The key as pasted, trimmed, or why it is not one. Only the characters
/// Nexus's keys use are accepted, so a pasted sentence is never stored or
/// sent.
fn clean_key(key: &str) -> AppResult<&str> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::validation(
            "NEXUS_KEY_EMPTY",
            "Paste your Nexus Mods API key first",
        ));
    }
    if key.len() > MAX_KEY_LEN
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'))
    {
        return Err(AppError::validation(
            "NEXUS_KEY_INVALID",
            "That is not a Nexus Mods API key. Copy the personal API key from your account's API Keys page",
        ));
    }
    Ok(key)
}

fn not_available() -> AppError {
    AppError::validation(
        "NEXUS_NOT_CONNECTED",
        "This build of the manager is not connected to Nexus Mods yet. Open the mod page and download the file there, then install the archive here",
    )
}

fn no_key() -> AppError {
    AppError::validation(
        "NEXUS_KEY_MISSING",
        "Add your Nexus Mods API key in Settings first",
    )
}

/// A download address with its query (Nexus's signed permission) left out,
/// for error details that may be kept or shared.
fn without_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

fn account_dto(account: &NexusAccount) -> NexusAccountDto {
    NexusAccountDto {
        name: account.name.clone(),
        is_premium: account.is_premium,
        hourly_remaining: account.hourly_remaining,
        daily_remaining: account.daily_remaining,
    }
}

struct WaitingLink {
    link: NxmLink,
    received: u64,
}

/// What a download is for, so the archive can be checked against it.
enum Expecting<'a> {
    /// An update of this installed mod to exactly this version.
    Update {
        item: &'a ModListItemDto,
        version: String,
    },
    /// A file the user chose on the site. It may be a mod already installed
    /// (any version) or a new one; nothing is required of it beyond being a
    /// mod archive.
    Chosen,
}

pub struct NexusService {
    api: Arc<dyn NexusApiPort>,
    keys: Arc<dyn SecretPort>,
    downloads: Arc<dyn ModDownloadPort>,
    mods: Arc<ModsService>,
    operations: Arc<OperationsService>,
    updates: Arc<ModUpdatesService>,
    /// Whether this build may use a personal API key at all.
    personal_key_allowed: bool,
    /// The saved key, read from the keychain once (None until then).
    key: Mutex<Option<Option<String>>>,
    rejected: AtomicBool,
    account: Mutex<Option<NexusAccount>>,
    links: Mutex<HashMap<String, WaitingLink>>,
    /// Why links that arrived were not taken, until the user has seen it.
    refused: Mutex<Vec<String>>,
}

impl NexusService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        api: Arc<dyn NexusApiPort>,
        keys: Arc<dyn SecretPort>,
        downloads: Arc<dyn ModDownloadPort>,
        mods: Arc<ModsService>,
        operations: Arc<OperationsService>,
        updates: Arc<ModUpdatesService>,
        personal_key_allowed: bool,
    ) -> Self {
        Self {
            api,
            keys,
            downloads,
            mods,
            operations,
            updates,
            personal_key_allowed,
            key: Mutex::new(None),
            rejected: AtomicBool::new(false),
            account: Mutex::new(None),
            links: Mutex::new(HashMap::new()),
            refused: Mutex::new(Vec::new()),
        }
    }

    pub fn personal_key_allowed(&self) -> bool {
        self.personal_key_allowed
    }

    /// Whether a key is saved and whose it is. Never the key itself. A build
    /// that may not use a personal key does not look in the keychain.
    pub fn status(&self) -> NexusStatusDto {
        if !self.personal_key_allowed {
            return NexusStatusDto {
                personal_key_allowed: false,
                key_set: false,
                rejected: false,
                keychain_error: None,
                account: None,
            };
        }
        let (key_set, keychain_error) = match self.keys.get() {
            Ok(key) => {
                let set = key.is_some();
                if let Ok(mut cached) = self.key.lock() {
                    *cached = Some(key);
                }
                (set, None)
            }
            Err(error) => (false, Some(error.summary)),
        };
        NexusStatusDto {
            personal_key_allowed: true,
            key_set,
            rejected: key_set && self.rejected.load(Ordering::SeqCst),
            keychain_error,
            account: key_set
                .then(|| {
                    self.account
                        .lock()
                        .ok()
                        .and_then(|a| a.as_ref().map(account_dto))
                })
                .flatten(),
        }
    }

    /// Asks Nexus whose `key` is and saves it only when Nexus accepts it.
    pub async fn connect(&self, key: &str) -> AppResult<NexusStatusDto> {
        if !self.personal_key_allowed {
            return Err(not_available());
        }
        let key = clean_key(key)?;
        let account = self.api.account(key).await.map_err(|error| {
            if error.code == NEXUS_KEY_REJECTED {
                AppError::validation(
                    NEXUS_KEY_REJECTED,
                    "Nexus Mods did not accept that key. Check it was copied whole and has not been deleted",
                )
            } else {
                error
            }
        })?;
        self.keys.set(key)?;
        if let Ok(mut cached) = self.key.lock() {
            *cached = Some(Some(key.to_string()));
        }
        if let Ok(mut saved) = self.account.lock() {
            *saved = Some(account);
        }
        self.rejected.store(false, Ordering::SeqCst);
        Ok(self.status())
    }

    /// Asks Nexus about the saved key again.
    pub async fn test(&self) -> AppResult<NexusStatusDto> {
        let key = self.saved_key()?;
        self.with_key(self.api.account(&key)).await.map(|account| {
            if let Ok(mut saved) = self.account.lock() {
                *saved = Some(account);
            }
        })?;
        Ok(self.status())
    }

    /// Removes the saved key.
    pub fn disconnect(&self) -> AppResult<NexusStatusDto> {
        if !self.personal_key_allowed {
            return Err(not_available());
        }
        self.keys.clear()?;
        if let Ok(mut cached) = self.key.lock() {
            *cached = Some(None);
        }
        if let Ok(mut saved) = self.account.lock() {
            *saved = None;
        }
        self.rejected.store(false, Ordering::SeqCst);
        Ok(self.status())
    }

    /// The saved key, or why there is none to use.
    fn saved_key(&self) -> AppResult<String> {
        if !self.personal_key_allowed {
            return Err(not_available());
        }
        let mut cached = self
            .key
            .lock()
            .map_err(|_| AppError::validation("NEXUS_KEY_MISSING", "The key could not be read"))?;
        if cached.is_none() {
            *cached = Some(self.keys.get()?);
        }
        cached.clone().flatten().ok_or_else(no_key)
    }

    /// Runs a Nexus read, noting when Nexus refuses the key so Settings can
    /// say so.
    async fn with_key<T>(
        &self,
        read: impl std::future::Future<Output = AppResult<T>>,
    ) -> AppResult<T> {
        let result = read.await;
        match &result {
            Err(error) if error.code == NEXUS_KEY_REJECTED => {
                self.rejected.store(true, Ordering::SeqCst);
                if let Ok(mut saved) = self.account.lock() {
                    *saved = None;
                }
            }
            Ok(_) => self.rejected.store(false, Ordering::SeqCst),
            _ => {}
        }
        result.map_err(|error| {
            if error.code == NEXUS_KEY_REJECTED {
                AppError::validation(
                    NEXUS_KEY_REJECTED,
                    "Nexus Mods refused your API key (it may have been deleted). Replace it in Settings",
                )
            } else {
                error
            }
        })
    }

    // --- nxm:// links -----------------------------------------------------

    /// Reads a link the site opened and keeps it until the user says what
    /// to do with it. `now` is seconds since 1970.
    pub fn receive_link(
        &self,
        raw: &str,
        now: u64,
        installed: &[ModListItemDto],
    ) -> AppResult<NxmLinkDto> {
        let link =
            NxmLink::parse(raw).map_err(|why| AppError::validation(why.code(), why.describe()))?;
        if link.is_expired(now) {
            return Err(AppError::validation(
                "NXM_LINK_EXPIRED",
                "That Nexus Mods download link has expired. Use \"Mod manager download\" on the mod's page again",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let dto = self.link_dto(&id, &link, installed);
        if let Ok(mut links) = self.links.lock() {
            links.retain(|_, waiting| !waiting.link.is_expired(now));
            while links.len() >= MAX_WAITING_LINKS {
                let Some(oldest) = links
                    .iter()
                    .min_by_key(|(_, w)| w.received)
                    .map(|(id, _)| id.clone())
                else {
                    break;
                };
                links.remove(&oldest);
            }
            links.insert(
                id,
                WaitingLink {
                    link,
                    received: now,
                },
            );
        }
        Ok(dto)
    }

    fn link_dto(&self, id: &str, link: &NxmLink, installed: &[ModListItemDto]) -> NxmLinkDto {
        NxmLinkDto {
            id: id.to_string(),
            mod_id: link.mod_id,
            file_id: link.file_id,
            page_url: link.page_url(),
            expires_at: link
                .expires
                .and_then(|secs| i64::try_from(secs).ok())
                .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
                .map(|at| at.to_rfc3339()),
            installed_name: installed_for(installed, link.mod_id).map(|m| m.name.clone()),
        }
    }

    /// Keeps why a link that arrived was not taken, for the window to show.
    pub fn note_refused_link(&self, why: &str) {
        if let Ok(mut refused) = self.refused.lock() {
            refused.push(why.to_string());
            let extra = refused.len().saturating_sub(MAX_WAITING_LINKS);
            refused.drain(..extra);
        }
    }

    /// The links waiting, newest first, and why any were refused. Links
    /// whose key has expired are dropped.
    pub fn waiting(&self, installed: &[ModListItemDto], now: u64) -> NxmInboxDto {
        let mut links: Vec<(u64, NxmLinkDto)> = self
            .links
            .lock()
            .map(|mut links| {
                links.retain(|_, waiting| !waiting.link.is_expired(now));
                links
                    .iter()
                    .map(|(id, w)| (w.received, self.link_dto(id, &w.link, installed)))
                    .collect()
            })
            .unwrap_or_default();
        links.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        NxmInboxDto {
            links: links.into_iter().map(|(_, dto)| dto).collect(),
            refused: self.refused.lock().map(|r| r.clone()).unwrap_or_default(),
        }
    }

    /// Forgets why earlier links were refused, once the user has seen it.
    pub fn clear_refused_links(&self) {
        if let Ok(mut refused) = self.refused.lock() {
            refused.clear();
        }
    }

    /// Forgets a link the user dismissed.
    pub fn dismiss_link(&self, id: &str) {
        if let Ok(mut links) = self.links.lock() {
            links.remove(id);
        }
    }

    /// Downloads the file a received link names and prepares it for install
    /// into `profile_id`. The link is used up either way: its key is for one
    /// download.
    pub async fn prepare_link(
        &self,
        profile_id: &ProfileId,
        link_id: &str,
        now: u64,
    ) -> AppResult<DownloadedPreviewDto> {
        let key = self.saved_key()?;
        let waiting = self
            .links
            .lock()
            .ok()
            .and_then(|mut links| links.remove(link_id))
            .ok_or_else(|| {
                AppError::validation(
                    "NXM_LINK_UNKNOWN",
                    "That download link is no longer waiting. Use \"Mod manager download\" on the mod's page again",
                )
            })?;
        let link = waiting.link;
        if link.is_expired(now) {
            return Err(AppError::validation(
                "NXM_LINK_EXPIRED",
                "That Nexus Mods download link has expired. Use \"Mod manager download\" on the mod's page again",
            ));
        }
        self.updates
            .one_download(profile_id, async {
                let files = self
                    .with_key(self.api.mod_files(&key, link.mod_id))
                    .await?;
                let file = files
                    .iter()
                    .find(|f| f.file_id == link.file_id)
                    .cloned()
                    .ok_or_else(|| {
                        AppError::validation(
                            "NEXUS_FILE_NOT_LISTED",
                            "Nexus does not list that file for the mod any more. It may have been removed; look on the mod's page",
                        )
                    })?;
                if !file.is_zip() {
                    return Err(AppError::validation(
                        "NEXUS_FILE_NOT_ZIP",
                        format!(
                            "{} is not a ZIP archive, and the manager installs ZIP archives only",
                            file.file_name
                        ),
                    ));
                }
                if file.size_bytes > manager_core::updates::MAX_RELEASE_ASSET_BYTES {
                    return Err(AppError::validation(
                        "NEXUS_FILE_TOO_LARGE",
                        format!("{} is larger than the manager downloads", file.file_name),
                    ));
                }
                let link_key = match (&link.key, link.expires) {
                    (Some(key), Some(expires)) => Some(NexusLinkKey {
                        key: key.clone(),
                        expires,
                    }),
                    _ => None,
                };
                let url = self
                    .with_key(self.api
                            .download_url(&key, link.mod_id, file.file_id, link_key.as_ref()),
                    )
                    .await
                    .map_err(premium_note)?;
                self.download_and_prepare(
                    profile_id,
                    &key,
                    link.mod_id,
                    &file,
                    &url,
                    Expecting::Chosen,
                )
                .await
            })
            .await
    }

    // --- Updates ----------------------------------------------------------

    /// Downloads from Nexus the update the last check found for one mod and
    /// prepares it for install. Only Premium members may download this way;
    /// anyone else is told to use the site's "Mod manager download" button.
    pub async fn prepare_update(
        &self,
        profile_id: &ProfileId,
        profile_component_id: &str,
        installed: &[ModListItemDto],
    ) -> AppResult<DownloadedPreviewDto> {
        let key = self.saved_key()?;
        let item = installed
            .iter()
            .find(|m| m.profile_component_id == profile_component_id)
            .ok_or_else(|| {
                AppError::validation("MOD_NOT_IN_PROFILE", "That mod is not in this profile")
            })?;
        let version = self
            .updates
            .cached(profile_id, installed)
            .mods
            .into_iter()
            .find(|m| m.profile_component_id == profile_component_id)
            .and_then(|m| m.update_version)
            .ok_or_else(|| {
                AppError::validation(
                    "NO_UPDATE_FOUND",
                    "No update is known for this mod. Check for updates first",
                )
            })?;
        let source = nexus_source(&item.update_keys).ok_or_else(|| {
            AppError::validation(
                "NO_NEXUS_PAGE",
                "The mod's manifest does not name a Nexus Mods page, so the manager cannot download it from Nexus. Open its mod page instead",
            )
        })?;
        self.updates
            .one_download(profile_id, async {
                let files = self
                    .with_key(self.api.mod_files(&key, source.mod_id))
                    .await?;
                let file = choose_nexus_file(&files, &version, source.subkey.as_deref())
                    .map_err(|why| AppError::validation("NO_NEXUS_FILE", why.describe(&version)))?
                    .clone();
                let url = self
                    .with_key(
                        self.api
                            .download_url(&key, source.mod_id, file.file_id, None),
                    )
                    .await
                    .map_err(premium_note)?;
                self.download_and_prepare(
                    profile_id,
                    &key,
                    source.mod_id,
                    &file,
                    &url,
                    Expecting::Update {
                        item,
                        version: version.clone(),
                    },
                )
                .await
            })
            .await
    }

    async fn download_and_prepare(
        &self,
        profile_id: &ProfileId,
        key: &str,
        mod_id: u64,
        file: &NexusFile,
        url: &str,
        expecting: Expecting<'_>,
    ) -> AppResult<DownloadedPreviewDto> {
        let path = self
            .downloads
            .download(url, None, file.size_bytes, &file.file_name)
            .await
            .map_err(|mut error| {
                // Nexus's download address carries a signed permission; it
                // is not kept in an error that may be saved or shared.
                error.technical_details = error
                    .technical_details
                    .map(|d| d.replace(url, without_query(url)));
                error
            })?;
        let md5 = match self.downloads.md5(&path) {
            Ok(md5) => md5,
            Err(error) => {
                self.downloads.discard(&path);
                return Err(error);
            }
        };
        // Nexus knows each file's MD5. A file it knows as another file is
        // not the one asked for; one it does not know (just uploaded, or the
        // lookup failed) is installed with that said in the review.
        let matches = self
            .with_key(self.api.find_by_md5(key, &md5))
            .await
            .unwrap_or_default();
        let md5_confirmed = matches
            .iter()
            .any(|m| m.mod_id == mod_id && m.file_id == file.file_id);
        if !md5_confirmed && !matches.is_empty() {
            self.downloads.discard(&path);
            return Err(AppError::validation(
                "NEXUS_FILE_MISMATCH",
                format!(
                    "The downloaded file is not {} as Nexus lists it, so it was not kept. Try again",
                    file.file_name
                ),
            ));
        }
        let page_url = mod_page_url(mod_id);
        let origin = DownloadOrigin {
            filename: file.file_name.clone(),
            expected_sha256: None,
            metadata: serde_json::json!({
                "provider": "nexus",
                "game": STARDEW_GAME_DOMAIN,
                "mod_id": mod_id,
                "file_id": file.file_id,
                "file_display_name": file.name,
                "version": file.version,
                "page_url": page_url,
                "md5": md5,
                "md5_confirmed": md5_confirmed,
                "for_unique_id": match &expecting {
                    Expecting::Update { item, .. } => Some(item.unique_id.clone()),
                    Expecting::Chosen => None,
                },
            }),
        };
        let result = self
            .mods
            .prepare_downloaded_install(profile_id, &path, &origin);
        self.downloads.discard(&path);
        let preview = result?;

        if let Expecting::Update { item, version } = &expecting {
            let holds_it = preview.detected_components.iter().any(|c| {
                c.unique_id.eq_ignore_ascii_case(&item.unique_id)
                    && same_version(&c.version, version)
            });
            if !holds_it {
                if let Ok(id) = OperationId::from_str(&preview.operation_id) {
                    let _ = self.operations.cancel_operation(&id);
                }
                let found = preview
                    .detected_components
                    .iter()
                    .map(|c| format!("{} {} ({})", c.name, c.version, c.unique_id))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(AppError::validation(
                    "UPDATE_ARCHIVE_MISMATCH",
                    format!(
                        "The downloaded file {} is not {} {}: it holds {}. Nothing was installed",
                        file.file_name,
                        item.name,
                        version,
                        if found.is_empty() { "no mod" } else { &found }
                    ),
                ));
            }
        }
        Ok(DownloadedPreviewDto {
            preview,
            checksum_verified: md5_confirmed,
            source: DownloadSourceDto::Nexus(NexusDownloadDto {
                mod_id,
                file_id: file.file_id,
                file_name: file.file_name.clone(),
                display_name: file.name.clone(),
                version: file.version.clone(),
                size_bytes: file.size_bytes,
                page_url,
                md5_confirmed,
            }),
        })
    }
}

/// The installed mod whose manifest names this Nexus page.
fn installed_for(installed: &[ModListItemDto], mod_id: u64) -> Option<&ModListItemDto> {
    installed
        .iter()
        .find(|m| nexus_source(&m.update_keys).is_some_and(|s| s.mod_id == mod_id))
}

/// Nexus's "Premium only" refusal, with what to do instead.
fn premium_note(error: AppError) -> AppError {
    if error.code == NEXUS_PREMIUM_REQUIRED {
        AppError::validation(
            NEXUS_PREMIUM_REQUIRED,
            "Nexus Mods lets only Premium members download through other apps directly. Open the mod page and use \"Mod manager download\" on the file instead; the manager picks it up from there",
        )
    } else {
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_checked_before_they_are_kept() {
        assert_eq!(clean_key("  abc+/=-_123 ").unwrap(), "abc+/=-_123");
        assert_eq!(clean_key("").unwrap_err().code, "NEXUS_KEY_EMPTY");
        assert_eq!(
            clean_key("my key is abc").unwrap_err().code,
            "NEXUS_KEY_INVALID"
        );
        assert_eq!(
            clean_key(&"a".repeat(MAX_KEY_LEN + 1)).unwrap_err().code,
            "NEXUS_KEY_INVALID"
        );
    }

    #[test]
    fn download_addresses_lose_their_permission_in_errors() {
        assert_eq!(
            without_query("https://cf-files.nexusmods.com/a/b.zip?md5=x&expires=1"),
            "https://cf-files.nexusmods.com/a/b.zip"
        );
        assert_eq!(without_query("https://x/y"), "https://x/y");
    }
}
