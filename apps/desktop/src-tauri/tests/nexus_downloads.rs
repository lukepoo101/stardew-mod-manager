//! Downloads from Nexus Mods: an update found by the check (Premium), and a
//! file chosen with the site's "Mod manager download" button (an `nxm://`
//! link). Both go through the same review and replace as a chosen archive.
//!
//! Nexus, the keychain and the download are stand-ins, so these run offline
//! and never need a real API key.
#![cfg(target_os = "linux")]
// The stand-ins return the app's own error type.
#![allow(clippy::result_large_err)]

use async_trait::async_trait;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::mod_sources::{
    GitHubRateLimit, GitHubReleasesPort, ModUpdateCheckPort, NexusAccount, NexusApiPort,
    NexusLinkKey, NexusMd5Match, SecretPort, NEXUS_KEY_REJECTED, NEXUS_PREMIUM_REQUIRED,
};
use manager_app::ports::repositories::{
    GameInstallationRepository, PackageCatalogRepository, SmapiRepository,
};
use manager_app::ports::runtime::DownloadPort;
use manager_app::services::{ModUpdatesService, NexusService};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{ArtifactHash, GameInstallationId, OperationId, ProfileId};
use manager_core::nexus::{NexusFile, NexusFileCategory};
use manager_core::smapi::ManagedSmapiInstallation;
use manager_core::updates::{GitHubRelease, ModUpdateInfo, ModUpdateQuery};
use manager_infra::paths::AppPaths;
use serde_json::Value;
use stardew_mod_manager::state::AppState;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

const KEY: &str = "PERSONAL+KEY/abc==--def";
const MOD_ID: u64 = 541;
const NOW: u64 = 1_800_000_000;

struct Setup {
    tmp: tempfile::TempDir,
    state: AppState,
    profile_id: ProfileId,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let state = AppState::new_with_expected_smapi_hash(paths, Some("test")).unwrap();
    let game_id = GameInstallationId::new();
    state
        .repo
        .save_game(&GameInstallation {
            id: game_id,
            canonical_root: tmp.path().join("game"),
            operating_system: OperatingSystem::Linux,
            storefront: Storefront::Steam,
            management_mode: ManagementMode::Managed,
            created_at: "2026-11-09T00:00:00Z".parse().unwrap(),
        })
        .unwrap();
    state.services.games.set_active_game(&game_id).unwrap();
    state
        .repo
        .save_smapi_installation(&ManagedSmapiInstallation {
            game_installation_id: game_id,
            release_version: "4.1.10".to_string(),
            release_policy_id: "pinned".to_string(),
            installed_at: "2026-11-09T00:00:00Z".parse().unwrap(),
        })
        .unwrap();
    let profile = state
        .services
        .profiles
        .create_profile(&game_id, "Main", None)
        .unwrap();
    let profile_id = ProfileId::from_str(&profile.id).unwrap();
    state
        .services
        .profiles
        .switch_active_profile(&game_id, &profile_id)
        .unwrap();
    Setup {
        tmp,
        state,
        profile_id,
    }
}

fn mod_zip(dir: &Path, file: &str, unique_id: &str, version: &str, keys: &[&str]) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(file);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    let folder = unique_id.rsplit('.').next().unwrap();
    zip.start_file(format!("{folder}/manifest.json"), options)
        .unwrap();
    write!(
        zip,
        r#"{{"Name":"{folder}","Author":"Tests","Version":"{version}","UniqueID":"{unique_id}","EntryDll":"{folder}.dll","UpdateKeys":{}}}"#,
        serde_json::to_string(keys).unwrap()
    )
    .unwrap();
    zip.start_file(format!("{folder}/{folder}.dll"), options)
        .unwrap();
    write!(zip, "{version}").unwrap();
    zip.finish().unwrap();
    path
}

fn install(s: &Setup, archive: &Path) {
    let preview = s
        .state
        .services
        .mods
        .prepare_install(&s.profile_id, archive)
        .unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    s.state
        .services
        .operations
        .commit_operation(&OperationId::from_str(&preview.operation_id).unwrap())
        .unwrap();
}

fn installed(s: &Setup) -> Vec<manager_app::api::dto::ModListItemDto> {
    s.state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap()
}

fn md5_of(path: &Path) -> String {
    manager_infra::mod_sources::file_md5(path).unwrap()
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tauri::async_runtime::block_on(future)
}

// --- Stand-ins -------------------------------------------------------------

#[derive(Default)]
struct FakeCheck(Mutex<Vec<ModUpdateInfo>>);

#[async_trait]
impl ModUpdateCheckPort for FakeCheck {
    async fn check(
        &self,
        _mods: &[ModUpdateQuery],
        _smapi: Option<&str>,
        _game: Option<&str>,
    ) -> AppResult<Vec<ModUpdateInfo>> {
        Ok(self.0.lock().unwrap().clone())
    }
}

struct NoGitHub;

#[async_trait]
impl GitHubReleasesPort for NoGitHub {
    async fn releases(&self, _repo: &str, _token: Option<&str>) -> AppResult<Vec<GitHubRelease>> {
        Ok(Vec::new())
    }
    async fn rate_limit(&self, _token: Option<&str>) -> AppResult<GitHubRateLimit> {
        Ok(GitHubRateLimit {
            limit: 60,
            remaining: 60,
            resets_at: 0,
        })
    }
}

/// The keychain, in memory, counting how often it is touched.
#[derive(Default)]
struct MemorySecret {
    value: Mutex<Option<String>>,
    touched: Mutex<usize>,
}

impl SecretPort for MemorySecret {
    fn get(&self) -> AppResult<Option<String>> {
        *self.touched.lock().unwrap() += 1;
        Ok(self.value.lock().unwrap().clone())
    }
    fn set(&self, secret: &str) -> AppResult<()> {
        *self.touched.lock().unwrap() += 1;
        *self.value.lock().unwrap() = Some(secret.to_string());
        Ok(())
    }
    fn clear(&self) -> AppResult<()> {
        *self.touched.lock().unwrap() += 1;
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}

/// Nexus, for mod 541 only.
#[derive(Default)]
struct FakeNexus {
    premium: Mutex<bool>,
    files: Mutex<Vec<NexusFile>>,
    /// MD5 → the files Nexus knows by it.
    md5s: Mutex<HashMap<String, Vec<NexusMd5Match>>>,
    /// The link keys download links were asked for with.
    asked_with: Mutex<Vec<Option<NexusLinkKey>>>,
    keys_seen: Mutex<Vec<String>>,
}

impl FakeNexus {
    fn url(file_id: u64) -> String {
        format!("https://cf-files.nexusmods.com/cdn/1303/{MOD_ID}/{file_id}.zip?md5=sig&expires=1")
    }
}

#[async_trait]
impl NexusApiPort for FakeNexus {
    async fn account(&self, api_key: &str) -> AppResult<NexusAccount> {
        self.keys_seen.lock().unwrap().push(api_key.to_string());
        if api_key != KEY {
            return Err(AppError::network(NEXUS_KEY_REJECTED, "refused"));
        }
        Ok(NexusAccount {
            user_id: 1,
            name: "Farmer".into(),
            is_premium: *self.premium.lock().unwrap(),
            hourly_remaining: Some(99),
            daily_remaining: None,
        })
    }

    async fn mod_files(&self, api_key: &str, mod_id: u64) -> AppResult<Vec<NexusFile>> {
        self.keys_seen.lock().unwrap().push(api_key.to_string());
        assert_eq!(mod_id, MOD_ID);
        Ok(self.files.lock().unwrap().clone())
    }

    async fn download_url(
        &self,
        api_key: &str,
        _mod_id: u64,
        file_id: u64,
        link_key: Option<&NexusLinkKey>,
    ) -> AppResult<String> {
        self.keys_seen.lock().unwrap().push(api_key.to_string());
        self.asked_with.lock().unwrap().push(link_key.cloned());
        if link_key.is_none() && !*self.premium.lock().unwrap() {
            return Err(AppError::network(NEXUS_PREMIUM_REQUIRED, "premium only"));
        }
        Ok(Self::url(file_id))
    }

    async fn find_by_md5(&self, _api_key: &str, md5: &str) -> AppResult<Vec<NexusMd5Match>> {
        Ok(self
            .md5s
            .lock()
            .unwrap()
            .get(md5)
            .cloned()
            .unwrap_or_default())
    }
}

/// Serves local files by URL.
#[derive(Default)]
struct FakeDownload(Mutex<HashMap<String, PathBuf>>);

#[async_trait]
impl DownloadPort for FakeDownload {
    async fn ensure_downloaded(
        &self,
        url: &str,
        sha: Option<&str>,
        dest: &Path,
    ) -> AppResult<PathBuf> {
        self.download_file(url, sha, dest).await
    }

    async fn download_file(
        &self,
        url: &str,
        _sha: Option<&str>,
        dest: &Path,
    ) -> AppResult<PathBuf> {
        let source = self.0.lock().unwrap().get(url).cloned().ok_or_else(|| {
            AppError::network("DOWNLOAD_HTTP_ERROR", "404")
                .with_details(format!("HTTP 404 for {url}"))
        })?;
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::copy(source, dest).unwrap();
        Ok(dest.to_path_buf())
    }
}

struct Fakes {
    check: Arc<FakeCheck>,
    nexus: Arc<FakeNexus>,
    keys: Arc<MemorySecret>,
    download: Arc<FakeDownload>,
    updates: Arc<ModUpdatesService>,
    service: NexusService,
}

fn fakes(s: &Setup, personal_key_allowed: bool) -> Fakes {
    let check = Arc::new(FakeCheck::default());
    let nexus = Arc::new(FakeNexus::default());
    let keys = Arc::new(MemorySecret::default());
    let download = Arc::new(FakeDownload::default());
    let downloads = Arc::new(manager_infra::mod_sources::ModDownloads::new(
        download.clone(),
        s.state.paths.mod_downloads_dir(),
    ));
    let updates = Arc::new(ModUpdatesService::new(
        s.state.repo.clone(),
        check.clone(),
        Arc::new(NoGitHub),
        Arc::new(MemorySecret::default()),
        downloads.clone(),
        s.state.services.mods.clone(),
        s.state.services.operations.clone(),
    ));
    let service = NexusService::new(
        nexus.clone(),
        keys.clone(),
        downloads,
        s.state.services.mods.clone(),
        s.state.services.operations.clone(),
        updates.clone(),
        personal_key_allowed,
    );
    Fakes {
        check,
        nexus,
        keys,
        download,
        updates,
        service,
    }
}

/// Lists `archive` on Nexus as file `file_id` of mod 541 at `version`, and
/// serves it. Nexus knows it by its MD5 unless `known` is false.
fn list_file(f: &Fakes, file_id: u64, version: &str, archive: &Path, known: bool) {
    let name = archive.file_name().unwrap().to_string_lossy().to_string();
    f.nexus.files.lock().unwrap().push(NexusFile {
        file_id,
        name: "Look".into(),
        version: Some(version.into()),
        category: NexusFileCategory::Main,
        file_name: name,
        size_bytes: std::fs::metadata(archive).unwrap().len(),
        uploaded_at: None,
    });
    f.download
        .0
        .lock()
        .unwrap()
        .insert(FakeNexus::url(file_id), archive.to_path_buf());
    if known {
        f.nexus.md5s.lock().unwrap().insert(
            md5_of(archive),
            vec![NexusMd5Match {
                mod_id: MOD_ID,
                file_id,
            }],
        );
    }
}

fn update_to(f: &Fakes, s: &Setup, version: &str) {
    *f.check.0.lock().unwrap() = vec![ModUpdateInfo {
        unique_id: "Tests.Look".into(),
        suggested_version: Some(version.into()),
        suggested_url: Some(format!(
            "https://www.nexusmods.com/stardewvalley/mods/{MOD_ID}"
        )),
        latest_version: Some(version.into()),
        ..Default::default()
    }];
    let view = block_on(
        f.updates
            .check(&s.profile_id, &installed(s), Some("4.1.10"), None),
    );
    assert_eq!(view.mods[0].nexus_mod_id, Some(MOD_ID));
    assert_eq!(view.mods[0].download, None);
}

fn connect(f: &Fakes, premium: bool) {
    *f.nexus.premium.lock().unwrap() = premium;
    let status = block_on(f.service.connect(KEY)).unwrap();
    assert!(status.key_set);
    assert_eq!(status.account.unwrap().is_premium, premium);
}

fn install_old(s: &Setup) {
    install(
        s,
        &mod_zip(
            &s.tmp.path().join("old"),
            "old.zip",
            "Tests.Look",
            "1.0.0",
            &["Nexus:541"],
        ),
    );
}

// --- Tests -----------------------------------------------------------------

#[test]
fn a_premium_member_downloads_an_update_and_history_names_the_nexus_file() {
    let s = setup();
    let f = fakes(&s, true);
    install_old(&s);
    let new = mod_zip(
        &s.tmp.path().join("nexus"),
        "Look-541-1-1-0.zip",
        "Tests.Look",
        "1.1.0",
        &["Nexus:541"],
    );
    list_file(&f, 10, "1.1.0", &new, true);
    update_to(&f, &s, "1.1.0");
    connect(&f, true);

    let component = installed(&s)[0].profile_component_id.clone();
    let prepared = block_on(
        f.service
            .prepare_update(&s.profile_id, &component, &installed(&s)),
    )
    .unwrap();
    assert!(prepared.checksum_verified);
    let nexus = prepared.source.nexus().expect("from Nexus");
    assert_eq!((nexus.mod_id, nexus.file_id), (MOD_ID, 10));
    assert!(nexus.md5_confirmed);
    assert_eq!(prepared.preview.replaces[0].direction, "upgrade");
    // Premium: no link key was needed.
    assert_eq!(*f.nexus.asked_with.lock().unwrap(), vec![None]);

    let hash = ArtifactHash::parse(prepared.preview.artifact_hash.clone()).unwrap();
    let acquisition = &s.state.repo.get_acquisitions_for_artifact(&hash).unwrap()[0];
    let metadata: Value =
        serde_json::from_str(acquisition.source_metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["provider"], "nexus");
    assert_eq!(metadata["mod_id"], MOD_ID);
    assert_eq!(metadata["file_id"], 10);
    assert_eq!(metadata["md5_confirmed"], true);
    // The API key is never recorded with the package.
    assert!(!acquisition
        .source_metadata
        .as_deref()
        .unwrap()
        .contains(KEY));

    s.state
        .services
        .operations
        .cancel_operation(&OperationId::from_str(&prepared.preview.operation_id).unwrap())
        .unwrap();
    let replaced = manager_app::services::ReinstallService::new(
        s.state.repo.clone(),
        s.state.services.packages.clone(),
        s.state.services.mods.clone(),
        s.state.services.operations.clone(),
        s.state.services.toggle.clone(),
        Arc::new(manager_infra::deployed_files::FilesystemDeployedFiles::new(
            s.state.paths.clone(),
        )),
    )
    .with_restore_points(s.state.repo.clone(), s.state.repo.clone())
    .replace(&s.profile_id, &prepared.preview.artifact_hash)
    .unwrap();
    assert_eq!(installed(&s)[0].version, "1.1.0");

    let points =
        manager_app::services::restore_points::stored_points(&*s.state.repo, &s.profile_id)
            .unwrap();
    let point = points
        .iter()
        .find(|p| Some(&p.id) == replaced.restore_point.as_ref())
        .unwrap();
    assert_eq!(
        point.label,
        "Before changing Look 1.0.0, from Nexus mod 541 1.1.0"
    );
    let ops = s
        .state
        .services
        .operations
        .list_operations(Some(&s.profile_id))
        .unwrap();
    let new_install = ops
        .iter()
        .find(|o| o.kind == "mod_install" && o.state == "succeeded" && o.title.contains("1.1.0"))
        .unwrap();
    assert_eq!(
        new_install.source.as_deref(),
        Some("Downloaded from Nexus Mods file 10 of mod 541 (Look-541-1-1-0.zip, version 1.1.0, MD5 confirmed by Nexus)")
    );
}

#[test]
fn without_premium_the_update_points_to_the_mod_manager_download_button() {
    let s = setup();
    let f = fakes(&s, true);
    install_old(&s);
    let new = mod_zip(
        &s.tmp.path().join("nexus"),
        "Look.zip",
        "Tests.Look",
        "1.1.0",
        &["Nexus:541"],
    );
    list_file(&f, 10, "1.1.0", &new, true);
    update_to(&f, &s, "1.1.0");
    connect(&f, false);
    let component = installed(&s)[0].profile_component_id.clone();
    let error = block_on(
        f.service
            .prepare_update(&s.profile_id, &component, &installed(&s)),
    )
    .unwrap_err();
    assert_eq!(error.code, NEXUS_PREMIUM_REQUIRED);
    assert!(
        error.summary.contains("Mod manager download"),
        "{}",
        error.summary
    );
    assert_eq!(installed(&s)[0].version, "1.0.0");
}

#[test]
fn a_file_that_is_not_the_mod_or_not_the_file_is_not_kept() {
    let s = setup();
    let f = fakes(&s, true);
    install_old(&s);
    connect(&f, true);
    // Listed as 1.1.0, but the archive holds 1.0.5.
    let wrong = mod_zip(
        &s.tmp.path().join("nexus"),
        "Look.zip",
        "Tests.Look",
        "1.0.5",
        &["Nexus:541"],
    );
    list_file(&f, 10, "1.1.0", &wrong, true);
    update_to(&f, &s, "1.1.0");
    let component = installed(&s)[0].profile_component_id.clone();
    let error = block_on(
        f.service
            .prepare_update(&s.profile_id, &component, &installed(&s)),
    )
    .unwrap_err();
    assert_eq!(error.code, "UPDATE_ARCHIVE_MISMATCH");

    // Nexus knows the downloaded file as another file: refused before it is
    // inspected or stored.
    let other = mod_zip(
        &s.tmp.path().join("other"),
        "Look2.zip",
        "Tests.Look",
        "1.1.0",
        &["Nexus:541"],
    );
    f.nexus.files.lock().unwrap().clear();
    list_file(&f, 11, "1.1.0", &other, false);
    f.nexus.md5s.lock().unwrap().insert(
        md5_of(&other),
        vec![NexusMd5Match {
            mod_id: 999,
            file_id: 1,
        }],
    );
    let error = block_on(
        f.service
            .prepare_update(&s.profile_id, &component, &installed(&s)),
    )
    .unwrap_err();
    assert_eq!(error.code, "NEXUS_FILE_MISMATCH");
    assert_eq!(installed(&s)[0].version, "1.0.0");
    assert_eq!(
        std::fs::read_dir(s.state.paths.mod_downloads_dir())
            .map(|d| d.count())
            .unwrap_or(0),
        0,
        "nothing is left in the downloads folder"
    );
}

#[test]
fn a_mod_manager_download_link_installs_without_premium_and_is_used_once() {
    let s = setup();
    let f = fakes(&s, true);
    connect(&f, false);
    let new = mod_zip(
        &s.tmp.path().join("nexus"),
        "Look.zip",
        "Tests.Look",
        "2.0.0",
        &["Nexus:541"],
    );
    list_file(&f, 12, "2.0.0", &new, false);

    let link = f
        .service
        .receive_link(
            "nxm://stardewvalley/mods/541/files/12?key=abc_DEF-1&expires=1800000600&user_id=1",
            NOW,
            &installed(&s),
        )
        .unwrap();
    assert_eq!((link.mod_id, link.file_id), (MOD_ID, 12));
    assert_eq!(link.installed_name, None);
    let waiting = f.service.waiting(&installed(&s), NOW);
    assert_eq!(waiting.links.len(), 1);
    // The window never sees the link's key.
    assert!(!serde_json::to_string(&waiting)
        .unwrap()
        .contains("abc_DEF-1"));

    let prepared = block_on(f.service.prepare_link(&s.profile_id, &link.id, NOW)).unwrap();
    // Nexus did not know its MD5: it is still offered, and says so.
    assert!(!prepared.checksum_verified);
    assert_eq!(
        *f.nexus.asked_with.lock().unwrap(),
        vec![Some(NexusLinkKey {
            key: "abc_DEF-1".into(),
            expires: 1_800_000_600
        })]
    );
    assert!(prepared.preview.replaces.is_empty());
    s.state
        .services
        .operations
        .commit_operation(&OperationId::from_str(&prepared.preview.operation_id).unwrap())
        .unwrap();
    assert_eq!(installed(&s)[0].version, "2.0.0");

    // Used up.
    assert!(f.service.waiting(&installed(&s), NOW).links.is_empty());
    let again = block_on(f.service.prepare_link(&s.profile_id, &link.id, NOW)).unwrap_err();
    assert_eq!(again.code, "NXM_LINK_UNKNOWN");
}

#[test]
fn expired_and_foreign_links_are_refused() {
    let s = setup();
    let f = fakes(&s, true);
    connect(&f, false);
    let expired = f
        .service
        .receive_link(
            "nxm://stardewvalley/mods/541/files/12?key=abc&expires=1700000000",
            NOW,
            &[],
        )
        .unwrap_err();
    assert_eq!(expired.code, "NXM_LINK_EXPIRED");
    let other = f
        .service
        .receive_link("nxm://skyrim/mods/1/files/2", NOW, &[])
        .unwrap_err();
    assert_eq!(other.code, "NXM_LINK_OTHER_GAME");

    // A link that expires while it waits is dropped.
    let link = f
        .service
        .receive_link(
            "nxm://stardewvalley/mods/541/files/12?key=abc&expires=1800000010",
            NOW,
            &[],
        )
        .unwrap();
    assert_eq!(f.service.waiting(&[], NOW).links.len(), 1);
    assert!(f.service.waiting(&[], NOW + 60).links.is_empty());
    let late = block_on(f.service.prepare_link(&s.profile_id, &link.id, NOW + 60)).unwrap_err();
    assert_eq!(late.code, "NXM_LINK_UNKNOWN");
}

#[test]
fn a_link_for_an_installed_mod_names_it() {
    let s = setup();
    let f = fakes(&s, true);
    install_old(&s);
    let link = f
        .service
        .receive_link("nxm://stardewvalley/mods/541/files/12", NOW, &installed(&s))
        .unwrap();
    assert_eq!(link.installed_name.as_deref(), Some("Look"));
}

#[test]
fn a_refused_key_is_not_saved() {
    let s = setup();
    let f = fakes(&s, true);
    let error = block_on(f.service.connect("WRONG-key")).unwrap_err();
    assert_eq!(error.code, NEXUS_KEY_REJECTED);
    assert_eq!(*f.keys.value.lock().unwrap(), None);
    assert_eq!(
        block_on(f.service.connect("not a key!")).unwrap_err().code,
        "NEXUS_KEY_INVALID"
    );
}

#[test]
fn a_build_without_the_personal_key_feature_never_reads_or_sends_a_key() {
    let s = setup();
    let f = fakes(&s, false);
    *f.keys.value.lock().unwrap() = Some(KEY.into());
    let status = f.service.status();
    assert!(!status.personal_key_allowed && !status.key_set);
    assert_eq!(
        block_on(f.service.connect(KEY)).unwrap_err().code,
        "NEXUS_NOT_CONNECTED"
    );
    install_old(&s);
    let component = installed(&s)[0].profile_component_id.clone();
    assert_eq!(
        block_on(
            f.service
                .prepare_update(&s.profile_id, &component, &installed(&s))
        )
        .unwrap_err()
        .code,
        "NEXUS_NOT_CONNECTED"
    );
    assert_eq!(
        *f.keys.touched.lock().unwrap(),
        0,
        "the keychain was never read"
    );
    assert!(f.nexus.keys_seen.lock().unwrap().is_empty());
}

#[test]
fn download_errors_do_not_keep_nexus_signed_address() {
    let s = setup();
    let f = fakes(&s, true);
    install_old(&s);
    connect(&f, true);
    // Listed, but the download server does not have it.
    f.nexus.files.lock().unwrap().push(NexusFile {
        file_id: 13,
        name: "Look".into(),
        version: Some("1.1.0".into()),
        category: NexusFileCategory::Main,
        file_name: "Look.zip".into(),
        size_bytes: 10,
        uploaded_at: None,
    });
    update_to(&f, &s, "1.1.0");
    let component = installed(&s)[0].profile_component_id.clone();
    let error = block_on(
        f.service
            .prepare_update(&s.profile_id, &component, &installed(&s)),
    )
    .unwrap_err();
    let details = error.technical_details.unwrap_or_default();
    assert!(details.contains("cf-files.nexusmods.com"), "{details}");
    assert!(!details.contains("md5=sig"), "{details}");
}
