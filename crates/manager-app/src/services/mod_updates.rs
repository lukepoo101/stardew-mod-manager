//! Checking a profile's mods for updates, and downloading an update from
//! GitHub into the same install review a chosen archive gets.
//!
//! Nothing here runs by itself: the check goes online only when asked, and
//! sends what SMAPI sends when the game starts (each mod's UniqueID, version
//! and update keys, plus the SMAPI and game versions). The last result is
//! kept per profile so the page can show it without going online again.
//!
//! A download is offered only for a GitHub release file that is exactly the
//! suggested version (see `manager_core::updates::choose_release_asset`). It
//! is checked against the SHA-256 GitHub publishes, where there is one,
//! inspected like any other archive, and the prepared install is discarded
//! unless the archive holds this mod at that version.

use crate::api::dto::{
    DownloadedPreviewDto, GitHubDownloadDto, ModListItemDto, ModUpdateDto, ModUpdatesDto,
};
use crate::error::{AppError, AppResult};
use crate::ports::mod_sources::{GitHubReleasesPort, ModDownloadPort, ModUpdateCheckPort};
use crate::ports::repositories::PreferencesRepository;
use crate::services::mods::ModsService;
use crate::services::operations::OperationsService;
use crate::services::packages::DownloadOrigin;
use chrono::Utc;
use manager_core::ids::{OperationId, ProfileId};
use manager_core::updates::{
    choose_release_asset, github_source, ChosenAsset, GitHubRelease, GitHubSource, ModUpdateInfo,
    ModUpdateQuery,
};
use manager_core::version::SmapiVersion;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// At most this many GitHub repositories are read per check, so a large
/// profile cannot spend GitHub's hourly allowance for requests without an
/// account (60) in one go. Repositories read recently are reused rather
/// than read again, so the next check reads the ones left out, and any of
/// them is read when its update is asked for.
const GITHUB_REPOS_PER_CHECK: usize = 20;

/// How many repositories are read at the same time.
const GITHUB_READS_AT_ONCE: usize = 4;

/// How long a repository's releases are reused before they are read again.
/// A download always reads them again.
const GITHUB_REUSE_FOR: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredCheck {
    checked_at: String,
    mods: Vec<ModUpdateDto>,
    /// Why the last check failed; its results are those of the one before.
    #[serde(default)]
    error: Option<String>,
}

fn key(profile_id: &ProfileId) -> String {
    format!("mod_updates.{profile_id}")
}

pub struct ModUpdatesService {
    preferences: Arc<dyn PreferencesRepository>,
    checker: Arc<dyn ModUpdateCheckPort>,
    github: Arc<dyn GitHubReleasesPort>,
    downloads: Arc<dyn ModDownloadPort>,
    mods: Arc<ModsService>,
    operations: Arc<OperationsService>,
    /// Releases read recently, by lower-case repository.
    recent_releases: Mutex<HashMap<String, (Instant, Vec<GitHubRelease>)>>,
    /// Profiles with a download being prepared, and the review the last
    /// download for each opened.
    preparing: Mutex<HashSet<String>>,
    open_reviews: Mutex<HashMap<String, String>>,
}

impl ModUpdatesService {
    pub fn new(
        preferences: Arc<dyn PreferencesRepository>,
        checker: Arc<dyn ModUpdateCheckPort>,
        github: Arc<dyn GitHubReleasesPort>,
        downloads: Arc<dyn ModDownloadPort>,
        mods: Arc<ModsService>,
        operations: Arc<OperationsService>,
    ) -> Self {
        Self {
            preferences,
            checker,
            github,
            downloads,
            mods,
            operations,
            recent_releases: Mutex::new(HashMap::new()),
            preparing: Mutex::new(HashSet::new()),
            open_reviews: Mutex::new(HashMap::new()),
        }
    }

    fn save(&self, profile_id: &ProfileId, stored: &StoredCheck) {
        if let Ok(json) = serde_json::to_string(stored) {
            let _ = self.preferences.set_preference(&key(profile_id), &json);
        }
    }

    /// Forgets a profile's results, as when the profile is deleted.
    pub fn forget(&self, profile_id: &ProfileId) {
        let _ = self.preferences.set_preference(&key(profile_id), "");
    }

    fn reusable_releases(&self, repo: &str) -> Option<Vec<GitHubRelease>> {
        let recent = self.recent_releases.lock().ok()?;
        recent
            .get(repo)
            .filter(|(at, _)| at.elapsed() < GITHUB_REUSE_FOR)
            .map(|(_, releases)| releases.clone())
    }

    async fn read_releases(&self, repo: &str) -> AppResult<Vec<GitHubRelease>> {
        let releases = self.github.releases(repo).await?;
        if let Ok(mut recent) = self.recent_releases.lock() {
            recent.insert(repo.to_lowercase(), (Instant::now(), releases.clone()));
        }
        Ok(releases)
    }

    fn stored(&self, profile_id: &ProfileId) -> StoredCheck {
        self.preferences
            .get_preference(&key(profile_id))
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// The last check, matched to the mods installed now: a mod removed
    /// since is left out, and an update already installed is no longer one.
    pub fn cached(&self, profile_id: &ProfileId, installed: &[ModListItemDto]) -> ModUpdatesDto {
        let stored = self.stored(profile_id);
        let by_id: HashMap<String, &ModUpdateDto> = stored
            .mods
            .iter()
            .map(|m| (m.unique_id.to_lowercase(), m))
            .collect();
        let mut mods = Vec::new();
        for item in installed {
            let Some(found) = by_id.get(&item.unique_id.to_lowercase()) else {
                continue;
            };
            let mut entry = (*found).clone();
            entry.profile_component_id = item.profile_component_id.clone();
            entry.name = item.name.clone();
            if entry.installed_version != item.version {
                entry.installed_version = item.version.clone();
                let still_newer = match (&entry.update_version, SmapiVersion::parse(&item.version))
                {
                    (Some(update), Ok(now)) => SmapiVersion::parse(update)
                        .map(|update| update > now)
                        .unwrap_or(update != &item.version),
                    (Some(update), Err(_)) => update != &item.version,
                    (None, _) => false,
                };
                if !still_newer {
                    entry.update_version = None;
                    entry.download = None;
                    entry.download_note = None;
                }
            }
            mods.push(entry);
        }
        sort(&mut mods);
        ModUpdatesDto {
            profile_id: profile_id.to_string(),
            checked_at: (!stored.checked_at.is_empty()).then_some(stored.checked_at),
            mods,
            error: stored.error,
        }
    }

    /// Checks every installed mod now. A failure keeps the last result and
    /// says why.
    pub async fn check(
        &self,
        profile_id: &ProfileId,
        installed: &[ModListItemDto],
        smapi_version: Option<&str>,
        game_version: Option<&str>,
    ) -> ModUpdatesDto {
        let mut seen = std::collections::HashSet::new();
        let queries: Vec<ModUpdateQuery> = installed
            .iter()
            .filter(|m| seen.insert(m.unique_id.to_lowercase()))
            .map(|m| ModUpdateQuery {
                unique_id: m.unique_id.clone(),
                installed_version: m.version.clone(),
                update_keys: m.update_keys.clone(),
            })
            .collect();
        if queries.is_empty() {
            let checked_at = Utc::now().to_rfc3339();
            self.save(
                profile_id,
                &StoredCheck {
                    checked_at: checked_at.clone(),
                    ..Default::default()
                },
            );
            return ModUpdatesDto {
                profile_id: profile_id.to_string(),
                checked_at: Some(checked_at),
                mods: Vec::new(),
                error: None,
            };
        }
        let infos = match self
            .checker
            .check(&queries, smapi_version, game_version)
            .await
        {
            Ok(infos) => infos,
            Err(error) => {
                // The results stay those of the last check that worked, and
                // the failure is kept with them so the page can say why.
                let mut stored = self.stored(profile_id);
                stored.error = Some(error.summary);
                self.save(profile_id, &stored);
                return self.cached(profile_id, installed);
            }
        };
        let by_id: HashMap<String, ModUpdateInfo> = infos
            .into_iter()
            .map(|mut info| {
                // Only a newer version is an update: a beta installed ahead
                // of the newest release is not offered the release.
                if let Some(installed) = queries
                    .iter()
                    .find(|q| q.unique_id.eq_ignore_ascii_case(&info.unique_id))
                {
                    if info
                        .suggested_version
                        .as_deref()
                        .is_some_and(|v| !is_newer(v, &installed.installed_version))
                    {
                        info.suggested_version = None;
                        info.suggested_url = None;
                    }
                }
                (info.unique_id.to_lowercase(), info)
            })
            .collect();

        // GitHub releases for the mods with an update, one read per
        // repository however many of its mods have one. Releases read
        // recently are reused, and do not count towards the limit.
        let mut wanted: BTreeSet<String> = BTreeSet::new();
        for item in installed {
            if let Some(info) = by_id.get(&item.unique_id.to_lowercase()) {
                if info.suggested_version.is_some() {
                    if let Some(source) =
                        github_source(&item.update_keys, info.github_repo.as_deref())
                    {
                        wanted.insert(source.repo.to_lowercase());
                    }
                }
            }
        }
        let mut releases: HashMap<String, Result<Vec<GitHubRelease>, String>> = HashMap::new();
        let mut to_read = Vec::new();
        for repo in wanted {
            match self.reusable_releases(&repo) {
                Some(list) => {
                    releases.insert(repo, Ok(list));
                }
                None => to_read.push(repo),
            }
        }
        to_read.truncate(GITHUB_REPOS_PER_CHECK);
        for batch in to_read.chunks(GITHUB_READS_AT_ONCE) {
            let reads = batch.iter().map(|repo| self.read_releases(repo));
            for (repo, read) in batch.iter().zip(futures_join_all(reads).await) {
                releases.insert(repo.clone(), read.map_err(|e| e.summary));
            }
        }

        let mut mods = Vec::new();
        for item in installed {
            let info = by_id
                .get(&item.unique_id.to_lowercase())
                .cloned()
                .unwrap_or_else(|| ModUpdateInfo {
                    unique_id: item.unique_id.clone(),
                    errors: vec!["The update check did not answer for this mod.".into()],
                    ..Default::default()
                });
            // A mod whose newest version was found was checked; errors from
            // the other sites it is listed on do not matter to the user.
            let mut errors = if info.latest_version.is_some() {
                Vec::new()
            } else {
                info.errors.clone()
            };
            if info.latest_version.is_none()
                && info.compatibility_status.is_none()
                && errors.is_empty()
            {
                errors.push(if item.update_keys.is_empty() {
                    "It lists no update keys, so the check cannot tell whether there is a newer version.".into()
                } else {
                    "The check found no version of it on the sites its update keys name.".into()
                });
            }
            let source = github_source(&item.update_keys, info.github_repo.as_deref());
            let mut github_unread = false;
            let (download, download_note) = match (&info.suggested_version, &source) {
                (Some(version), Some(source)) => match releases.get(&source.repo.to_lowercase()) {
                    // A repository with no releases publishes elsewhere;
                    // there is nothing to say beyond "use the mod page".
                    Some(Ok(list)) if list.iter().all(|r| r.draft) => (None, None),
                    Some(Ok(list)) => {
                        match choose_release_asset(
                            source,
                            list,
                            version,
                            &item.name,
                            &item.unique_id,
                        ) {
                            Ok(chosen) => (Some(download_dto(&chosen, source)), None),
                            Err(why) => (None, Some(why.describe(version))),
                        }
                    }
                    Some(Err(error)) => {
                        github_unread = true;
                        (None, Some(error.clone()))
                    }
                    None => {
                        github_unread = true;
                        (
                            None,
                            Some("GitHub was not asked during this check, to stay within its limit for requests without an account. Download looks it up now, or the next check will.".into()),
                        )
                    }
                },
                _ => (None, None),
            };
            mods.push(ModUpdateDto {
                profile_component_id: item.profile_component_id.clone(),
                unique_id: item.unique_id.clone(),
                name: item.name.clone(),
                installed_version: item.version.clone(),
                update_version: info.suggested_version.clone(),
                update_url: info.suggested_url.clone(),
                compatibility_status: info.compatibility_status.clone(),
                compatibility_summary: info.compatibility_summary.clone(),
                download,
                download_note,
                github_repo: source.map(|s| s.repo),
                github_unread,
                errors,
            });
        }
        sort(&mut mods);
        let checked_at = Utc::now().to_rfc3339();
        self.save(
            profile_id,
            &StoredCheck {
                checked_at: checked_at.clone(),
                mods: mods.clone(),
                error: None,
            },
        );
        ModUpdatesDto {
            profile_id: profile_id.to_string(),
            checked_at: Some(checked_at),
            mods,
            error: None,
        }
    }

    /// Downloads the update the last check found for one mod and prepares
    /// it for install. The result is reviewed and confirmed like any
    /// archive; nothing in the profile changes here.
    pub async fn prepare_update(
        &self,
        profile_id: &ProfileId,
        profile_component_id: &str,
        installed: &[ModListItemDto],
    ) -> AppResult<DownloadedPreviewDto> {
        let profile_key = profile_id.to_string();
        let started = self
            .preparing
            .lock()
            .map(|mut p| p.insert(profile_key.clone()))
            .unwrap_or(false);
        if !started {
            return Err(AppError::validation(
                "UPDATE_ALREADY_DOWNLOADING",
                "An update for this profile is already downloading. Wait for it to finish",
            ));
        }
        let result = self
            .prepare_update_now(profile_id, profile_component_id, installed)
            .await;
        if let Ok(mut p) = self.preparing.lock() {
            p.remove(&profile_key);
        }
        let downloaded = result?;
        // Only one downloaded update waits for review at a time: an earlier
        // one still open is closed, as the window shows only the newest.
        let earlier =
            self.open_reviews.lock().ok().and_then(|mut open| {
                open.insert(profile_key, downloaded.preview.operation_id.clone())
            });
        if let Some(earlier) = earlier.and_then(|id| OperationId::from_str(&id).ok()) {
            if let Ok(Some(op)) = self.operations.get_operation(&earlier) {
                if op.state.eq_ignore_ascii_case("draft") {
                    let _ = self.operations.cancel_operation(&earlier);
                }
            }
        }
        Ok(downloaded)
    }

    async fn prepare_update_now(
        &self,
        profile_id: &ProfileId,
        profile_component_id: &str,
        installed: &[ModListItemDto],
    ) -> AppResult<DownloadedPreviewDto> {
        let item = installed
            .iter()
            .find(|m| m.profile_component_id == profile_component_id)
            .ok_or_else(|| {
                AppError::validation("MOD_NOT_IN_PROFILE", "That mod is not in this profile")
            })?;
        let view = self.cached(profile_id, installed);
        let entry = view
            .mods
            .iter()
            .find(|m| m.profile_component_id == profile_component_id)
            .filter(|m| m.update_version.is_some())
            .ok_or_else(|| {
                AppError::validation(
                    "NO_UPDATE_FOUND",
                    "No update is known for this mod. Check for updates first",
                )
            })?;
        let version = entry.update_version.clone().unwrap_or_default();
        let listed_repo = entry
            .github_repo
            .clone()
            .or_else(|| entry.download.as_ref().map(|d| d.repo.clone()));
        let source: GitHubSource = github_source(&item.update_keys, listed_repo.as_deref())
            .ok_or_else(|| {
                AppError::validation(
                    "NO_GITHUB_RELEASE",
                    "This update is not published on GitHub, so the manager cannot download it. Open its mod page instead",
                )
            })?;

        // The release is read again rather than trusting the stored one, so
        // a file replaced or withdrawn since the check is noticed.
        let releases = self.read_releases(&source.repo).await?;
        let chosen =
            choose_release_asset(&source, &releases, &version, &item.name, &item.unique_id)
                .map_err(|why| AppError::validation("NO_GITHUB_RELEASE", why.describe(&version)))?;

        let path = self
            .downloads
            .download(
                &chosen.asset.download_url,
                chosen.asset.sha256.as_deref(),
                chosen.asset.size,
                &chosen.asset.name,
            )
            .await?;
        let origin = DownloadOrigin {
            filename: chosen.asset.name.clone(),
            expected_sha256: chosen.asset.sha256.clone(),
            metadata: serde_json::json!({
                "provider": "github",
                "repo": chosen.repo,
                "tag": chosen.tag,
                "release_url": chosen.release_url,
                "asset_url": chosen.asset.download_url,
                "for_unique_id": item.unique_id,
                "for_version": version,
            }),
        };
        let result = self
            .mods
            .prepare_downloaded_install(profile_id, &path, &origin);
        self.downloads.discard(&path);
        let preview = result?;

        // The archive must be the mod it was downloaded for, at the version
        // it was downloaded as.
        let holds_it = preview.detected_components.iter().any(|c| {
            c.unique_id.eq_ignore_ascii_case(&item.unique_id) && same_version(&c.version, &version)
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
                    chosen.asset.name,
                    item.name,
                    version,
                    if found.is_empty() { "no mod" } else { &found }
                ),
            ));
        }
        Ok(DownloadedPreviewDto {
            preview,
            checksum_verified: chosen.asset.sha256.is_some(),
            source: download_dto(&chosen, &source),
        })
    }
}

fn download_dto(chosen: &ChosenAsset, source: &GitHubSource) -> GitHubDownloadDto {
    GitHubDownloadDto {
        repo: chosen.repo.clone(),
        tag: chosen.tag.clone(),
        release_url: chosen.release_url.clone(),
        asset_name: chosen.asset.name.clone(),
        size_bytes: chosen.asset.size,
        has_checksum: chosen.asset.sha256.is_some(),
        declared_by_mod: source.declared_by_mod,
    }
}

/// Updates first, then by name.
fn sort(mods: &mut [ModUpdateDto]) {
    mods.sort_by(|a, b| {
        b.update_version
            .is_some()
            .cmp(&a.update_version.is_some())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Whether `candidate` is a newer version than `installed`. Versions that do
/// not parse are newer when they differ.
fn is_newer(candidate: &str, installed: &str) -> bool {
    match (
        SmapiVersion::parse(candidate),
        SmapiVersion::parse(installed),
    ) {
        (Ok(c), Ok(i)) => c > i,
        _ => !candidate.trim().eq_ignore_ascii_case(installed.trim()),
    }
}

/// Runs the reads together and returns their results in order.
async fn futures_join_all<F, T>(reads: impl Iterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T>,
{
    let mut pending: Vec<std::pin::Pin<Box<F>>> = reads.map(Box::pin).collect();
    let mut results: Vec<Option<T>> = pending.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut all_done = true;
        for (i, read) in pending.iter_mut().enumerate() {
            if results[i].is_none() {
                match read.as_mut().poll(cx) {
                    std::task::Poll::Ready(value) => results[i] = Some(value),
                    std::task::Poll::Pending => all_done = false,
                }
            }
        }
        if all_done {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
    results.into_iter().flatten().collect()
}

/// "1.0" and "1.0.0" are the same version; text is compared when either
/// does not parse.
fn same_version(a: &str, b: &str) -> bool {
    match (SmapiVersion::parse(a), SmapiVersion::parse(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_smapi_versions() {
        assert!(same_version("1.0", "1.0.0"));
        assert!(same_version("2.0.0", "2.0.0"));
        assert!(!same_version("2.0.0", "2.0.1"));
        assert!(same_version("weird", "WEIRD"));
        assert!(is_newer("1.2.0", "1.1.9"));
        assert!(!is_newer("1.2.0", "1.3.0-beta.1"));
        assert!(!is_newer("2.0", "2.0.0"));
    }
}
