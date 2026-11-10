//! Mod updates: the check (smapi.io), choosing a GitHub release file, and the
//! download going through the same review and replace as a chosen archive.
//!
//! The first tests use stand-ins for smapi.io, GitHub and the download, so
//! they run offline. `live_update_from_github` uses the real services with
//! real mods and is ignored by default:
//!
//!     cargo test -p stardew-mod-manager --test mod_updates -- --ignored
#![cfg(target_os = "linux")]
// The stand-ins return the app's own error type.
#![allow(clippy::result_large_err)]

use async_trait::async_trait;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::mod_sources::{
    GitHubRateLimit, GitHubReleasesPort, GitHubTokenPort, ModUpdateCheckPort,
};
use manager_app::ports::repositories::{
    GameInstallationRepository, OperationRepository, PackageCatalogRepository, SmapiRepository,
};
use manager_app::ports::runtime::DownloadPort;
use manager_app::services::ModUpdatesService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{ArtifactHash, GameInstallationId, OperationId, ProfileId};
use manager_core::package::AcquisitionSource;
use manager_core::smapi::ManagedSmapiInstallation;
use manager_core::updates::{GitHubAsset, GitHubRelease, ModUpdateInfo, ModUpdateQuery};
use manager_infra::paths::AppPaths;
use serde_json::{json, Value};
use stardew_mod_manager::state::AppState;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

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
            created_at: "2026-11-06T00:00:00Z".parse().unwrap(),
        })
        .unwrap();
    state.services.games.set_active_game(&game_id).unwrap();
    state
        .repo
        .save_smapi_installation(&ManagedSmapiInstallation {
            game_installation_id: game_id,
            release_version: "4.1.10".to_string(),
            release_policy_id: "pinned".to_string(),
            installed_at: "2026-11-06T00:00:00Z".parse().unwrap(),
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

/// A mod archive, as an author would publish it.
fn mod_zip(dir: &Path, file: &str, unique_id: &str, version: &str, keys: &[&str]) -> PathBuf {
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

fn reinstall(state: &AppState) -> manager_app::services::ReinstallService {
    manager_app::services::ReinstallService::new(
        state.repo.clone(),
        state.services.packages.clone(),
        state.services.mods.clone(),
        state.services.operations.clone(),
        state.services.toggle.clone(),
        Arc::new(manager_infra::deployed_files::FilesystemDeployedFiles::new(
            state.paths.clone(),
        )),
    )
}

fn installed_version(s: &Setup, unique_id: &str) -> String {
    s.state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap()
        .into_iter()
        .find(|m| m.unique_id.eq_ignore_ascii_case(unique_id))
        .map(|m| m.version)
        .unwrap_or_default()
}

// --- Stand-ins -------------------------------------------------------------

#[derive(Default)]
struct FakeCheck {
    answers: Mutex<Option<AppResult<Vec<ModUpdateInfo>>>>,
    asked: Mutex<Vec<ModUpdateQuery>>,
}

#[async_trait]
impl ModUpdateCheckPort for FakeCheck {
    async fn check(
        &self,
        mods: &[ModUpdateQuery],
        _smapi: Option<&str>,
        _game: Option<&str>,
    ) -> AppResult<Vec<ModUpdateInfo>> {
        self.asked.lock().unwrap().extend_from_slice(mods);
        self.answers
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Ok(Vec::new()))
    }
}

#[derive(Default)]
struct FakeGitHub {
    releases: Mutex<HashMap<String, Vec<GitHubRelease>>>,
    reads: Mutex<usize>,
    unreachable: Mutex<bool>,
}

#[async_trait]
impl GitHubReleasesPort for FakeGitHub {
    async fn rate_limit(&self, _token: Option<&str>) -> AppResult<GitHubRateLimit> {
        Ok(GitHubRateLimit {
            limit: 60,
            remaining: 60,
            resets_at: 0,
        })
    }

    async fn releases(&self, repo: &str, _token: Option<&str>) -> AppResult<Vec<GitHubRelease>> {
        *self.reads.lock().unwrap() += 1;
        if *self.unreachable.lock().unwrap() {
            return Err(AppError::network(
                "GITHUB_RELEASES_UNREACHABLE",
                "GitHub could not be reached.",
            ));
        }
        self.releases
            .lock()
            .unwrap()
            .get(&repo.to_lowercase())
            .cloned()
            .ok_or_else(|| AppError::network("GITHUB_RELEASES_UNREACHABLE", "not found"))
    }
}

/// The keychain, in memory.
#[derive(Default)]
struct MemoryToken(Mutex<Option<String>>);

impl GitHubTokenPort for MemoryToken {
    fn get(&self) -> AppResult<Option<String>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn set(&self, token: &str) -> AppResult<()> {
        *self.0.lock().unwrap() = Some(token.to_string());
        Ok(())
    }
    fn clear(&self) -> AppResult<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

/// Serves local files by URL, checking the checksum as the real one does.
#[derive(Default)]
struct FakeDownload {
    files: Mutex<HashMap<String, PathBuf>>,
}

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

    async fn download_file(&self, url: &str, sha: Option<&str>, dest: &Path) -> AppResult<PathBuf> {
        let source = self
            .files
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .ok_or_else(|| AppError::network("DOWNLOAD_HTTP_ERROR", "404"))?;
        let bytes = std::fs::read(&source).unwrap();
        if let Some(expected) = sha {
            use sha2::Digest;
            let actual = manager_core::ids::hash_to_hex(sha2::Sha256::digest(&bytes));
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(AppError::validation("DOWNLOAD_HASH_MISMATCH", "mismatch"));
            }
        }
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, bytes).unwrap();
        Ok(dest.to_path_buf())
    }
}

struct Fakes {
    check: Arc<FakeCheck>,
    github: Arc<FakeGitHub>,
    download: Arc<FakeDownload>,
    service: ModUpdatesService,
}

fn fakes(s: &Setup) -> Fakes {
    let check = Arc::new(FakeCheck::default());
    let github = Arc::new(FakeGitHub::default());
    let download = Arc::new(FakeDownload::default());
    let service = ModUpdatesService::new(
        s.state.repo.clone(),
        check.clone(),
        github.clone(),
        Arc::new(MemoryToken::default()),
        Arc::new(manager_infra::mod_sources::ModDownloads::new(
            download.clone(),
            s.state.paths.mod_downloads_dir(),
        )),
        s.state.services.mods.clone(),
        s.state.services.operations.clone(),
    );
    Fakes {
        check,
        github,
        download,
        service,
    }
}

fn update_info(unique_id: &str, version: &str, repo: Option<&str>) -> ModUpdateInfo {
    ModUpdateInfo {
        unique_id: unique_id.into(),
        suggested_version: Some(version.into()),
        suggested_url: Some("https://www.nexusmods.com/stardewvalley/mods/1".into()),
        latest_version: Some(version.into()),
        github_repo: repo.map(str::to_string),
        compatibility_status: Some("Ok".into()),
        ..Default::default()
    }
}

/// Publishes `archive` as a release file and returns its URL.
fn publish(f: &Fakes, repo: &str, tag: &str, archive: &Path, sha256: Option<String>) -> String {
    let name = archive.file_name().unwrap().to_string_lossy().to_string();
    let url = format!("https://github.com/{repo}/releases/download/{tag}/{name}");
    f.download
        .files
        .lock()
        .unwrap()
        .insert(url.clone(), archive.to_path_buf());
    f.github
        .releases
        .lock()
        .unwrap()
        .entry(repo.to_lowercase())
        .or_default()
        .push(GitHubRelease {
            tag: tag.into(),
            name: None,
            html_url: format!("https://github.com/{repo}/releases/tag/{tag}"),
            draft: false,
            prerelease: false,
            published_at: None,
            assets: vec![GitHubAsset {
                name,
                size: std::fs::metadata(archive).unwrap().len(),
                download_url: url.clone(),
                sha256,
            }],
        });
    url
}

fn sha256_of(path: &Path) -> String {
    use sha2::Digest;
    manager_core::ids::hash_to_hex(sha2::Sha256::digest(std::fs::read(path).unwrap()))
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tauri::async_runtime::block_on(future)
}

// --- Offline ---------------------------------------------------------------

/// The whole path: check, choose the release file, download it, review it as
/// an upgrade, replace the installed version, and the check no longer
/// offers it.
#[test]
fn an_update_found_on_github_replaces_the_installed_version() {
    let s = setup();
    let f = fakes(&s);
    let published = s.tmp.path().join("published");
    std::fs::create_dir_all(&published).unwrap();
    install(
        &s,
        &mod_zip(
            s.tmp.path(),
            "old.zip",
            "Tests.Look",
            "1.0.0",
            &["GitHub:tests/look"],
        ),
    );
    let new = mod_zip(
        &published,
        "Look.1.1.0.zip",
        "Tests.Look",
        "1.1.0",
        &["GitHub:tests/look"],
    );
    publish(&f, "tests/look", "v1.1.0", &new, Some(sha256_of(&new)));
    *f.check.answers.lock().unwrap() = Some(Ok(vec![update_info("Tests.Look", "1.1.0", None)]));

    let installed = s
        .state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap();
    let view = block_on(
        f.service
            .check(&s.profile_id, &installed, Some("4.1.10"), None),
    );
    assert_eq!(view.error, None);
    let look = &view.mods[0];
    assert_eq!(look.update_version.as_deref(), Some("1.1.0"));
    let download = look.download.as_ref().expect("a download is offered");
    assert_eq!(download.asset_name, "Look.1.1.0.zip");
    assert!(download.has_checksum && download.declared_by_mod);
    // What was sent is what SMAPI sends: the ID, version and update keys.
    let asked = f.check.asked.lock().unwrap().clone();
    assert_eq!(asked[0].unique_id, "Tests.Look");
    assert_eq!(asked[0].installed_version, "1.0.0");
    assert_eq!(asked[0].update_keys, vec!["GitHub:tests/look"]);

    let prepared = block_on(f.service.prepare_update(
        &s.profile_id,
        &look.profile_component_id,
        &installed,
    ))
    .unwrap();
    assert!(prepared.checksum_verified);
    assert_eq!(prepared.source.github().unwrap().tag, "v1.1.0");
    assert_eq!(prepared.preview.replaces.len(), 1);
    assert_eq!(prepared.preview.replaces[0].direction, "upgrade");
    // The download's temporary copy is gone; the package is stored.
    assert_eq!(
        std::fs::read_dir(s.state.paths.mod_downloads_dir())
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );
    // Where it came from is recorded with it.
    let hash = ArtifactHash::parse(prepared.preview.artifact_hash.clone()).unwrap();
    let acquisitions = s.state.repo.get_acquisitions_for_artifact(&hash).unwrap();
    assert_eq!(acquisitions[0].source, AcquisitionSource::Provider);
    assert_eq!(acquisitions[0].original_filename, "Look.1.1.0.zip");
    let metadata: Value =
        serde_json::from_str(acquisitions[0].source_metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["provider"], "github");
    assert_eq!(metadata["repo"], "tests/look");

    // Confirmed the way the review confirms a replacement.
    s.state
        .services
        .operations
        .cancel_operation(&OperationId::from_str(&prepared.preview.operation_id).unwrap())
        .unwrap();
    let replaced = reinstall(&s.state)
        .with_restore_points(s.state.repo.clone(), s.state.repo.clone())
        .replace(&s.profile_id, &prepared.preview.artifact_hash)
        .unwrap();
    assert_eq!(installed_version(&s, "Tests.Look"), "1.1.0");

    // The restore point taken first, and the install in Activity, say which
    // GitHub release it came from and that its checksum was verified.
    let points =
        manager_app::services::restore_points::stored_points(&*s.state.repo, &s.profile_id)
            .unwrap();
    let point = points
        .iter()
        .find(|p| Some(&p.id) == replaced.restore_point.as_ref())
        .expect("a restore point was saved");
    assert_eq!(
        point.label,
        "Before changing Look 1.0.0, from GitHub tests/look v1.1.0"
    );
    assert_eq!(
        point.source.as_deref(),
        Some("Taken before updating from GitHub release v1.1.0 of tests/look (Look.1.1.0.zip, checksum verified)")
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
        .expect("the new version's install is in Activity");
    assert_eq!(
        new_install.source.as_deref(),
        Some("Downloaded from GitHub release v1.1.0 of tests/look (Look.1.1.0.zip, checksum verified)")
    );
    let old_install = ops
        .iter()
        .find(|o| o.kind == "mod_install" && o.title.contains("1.0.0"))
        .unwrap();
    assert_eq!(old_install.source, None, "a chosen archive has no source");
    let dll = s
        .state
        .paths
        .profile_mods_dir(&s.profile_id)
        .join("Tests.Look/Look.dll");
    assert_eq!(std::fs::read(dll).unwrap(), b"1.1.0");

    // The stored check now matches what is installed.
    let installed = s
        .state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap();
    let after = f.service.cached(&s.profile_id, &installed);
    assert_eq!(after.mods[0].installed_version, "1.1.0");
    assert_eq!(after.mods[0].update_version, None);
    assert!(after.mods[0].download.is_none());
}

/// A file that is not the mod it was downloaded for is not offered for
/// install, and leaves nothing prepared.
#[test]
fn a_release_file_holding_another_version_is_refused() {
    let s = setup();
    let f = fakes(&s);
    let published = s.tmp.path().join("published");
    std::fs::create_dir_all(&published).unwrap();
    install(
        &s,
        &mod_zip(
            s.tmp.path(),
            "old.zip",
            "Tests.Look",
            "1.0.0",
            &["GitHub:tests/look"],
        ),
    );
    // Named 1.1.0, but holds 1.0.5.
    let wrong = mod_zip(&published, "Look.1.1.0.zip", "Tests.Look", "1.0.5", &[]);
    publish(&f, "tests/look", "v1.1.0", &wrong, None);
    *f.check.answers.lock().unwrap() = Some(Ok(vec![update_info("Tests.Look", "1.1.0", None)]));
    let installed = s
        .state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap();
    let view = block_on(f.service.check(&s.profile_id, &installed, None, None));
    let error = block_on(f.service.prepare_update(
        &s.profile_id,
        &view.mods[0].profile_component_id,
        &installed,
    ))
    .unwrap_err();
    assert_eq!(error.code, "UPDATE_ARCHIVE_MISMATCH");
    assert!(error.summary.contains("1.0.5"), "{}", error.summary);
    assert!(
        s.state
            .repo
            .list_unresolved_operations()
            .unwrap()
            .is_empty(),
        "the prepared install was discarded"
    );
    assert_eq!(installed_version(&s, "Tests.Look"), "1.0.0");

    // A checksum that does not match stops the download itself.
    let other = mod_zip(&published, "Look.1.1.0.zip", "Tests.Look", "1.1.0", &[]);
    f.github.releases.lock().unwrap().clear();
    publish(&f, "tests/look", "v1.1.0", &other, Some("0".repeat(64)));
    let error = block_on(f.service.prepare_update(
        &s.profile_id,
        &view.mods[0].profile_component_id,
        &installed,
    ))
    .unwrap_err();
    assert_eq!(error.code, "DOWNLOAD_HASH_MISMATCH");
    assert!(s
        .state
        .repo
        .list_unresolved_operations()
        .unwrap()
        .is_empty());
}

/// Only the mod's own key or the compatibility list's repository is used,
/// a shared repository's release must be named for the mod, and a mod
/// without either only gets its page.
#[test]
fn downloads_are_offered_only_when_the_release_is_clearly_the_mod() {
    let s = setup();
    let f = fakes(&s);
    let published = s.tmp.path().join("published");
    std::fs::create_dir_all(&published).unwrap();
    install(
        &s,
        &mod_zip(s.tmp.path(), "a.zip", "Tests.Kokoro", "2.0.0", &["Nexus:1"]),
    );
    install(
        &s,
        &mod_zip(s.tmp.path(), "b.zip", "Tests.Fluent", "1.0.0", &["Nexus:2"]),
    );
    install(
        &s,
        &mod_zip(s.tmp.path(), "c.zip", "Tests.Plain", "1.0.0", &["Nexus:3"]),
    );
    // One repository, several mods' releases; only Kokoro's carries 3.0.0
    // and Fluent's version exists only as another mod's release.
    let kokoro = mod_zip(&published, "Kokoro.3.0.0.zip", "Tests.Kokoro", "3.0.0", &[]);
    publish(&f, "tests/mods", "release/kokoro/3.0.0", &kokoro, None);
    let other = mod_zip(&published, "Other.2.0.0.zip", "Tests.Other", "2.0.0", &[]);
    publish(&f, "tests/mods", "release/other/2.0.0", &other, None);
    *f.check.answers.lock().unwrap() = Some(Ok(vec![
        update_info("Tests.Kokoro", "3.0.0", Some("tests/mods")),
        update_info("Tests.Fluent", "2.0.0", Some("tests/mods")),
        update_info("Tests.Plain", "1.1.0", None),
    ]));
    let installed = s
        .state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap();
    let view = block_on(f.service.check(&s.profile_id, &installed, None, None));
    let by_id = |id: &str| view.mods.iter().find(|m| m.unique_id == id).unwrap();

    let kokoro = by_id("Tests.Kokoro");
    assert_eq!(
        kokoro.download.as_ref().unwrap().asset_name,
        "Kokoro.3.0.0.zip"
    );
    assert!(!kokoro.download.as_ref().unwrap().declared_by_mod);
    let fluent = by_id("Tests.Fluent");
    assert!(fluent.download.is_none());
    assert!(
        fluent
            .download_note
            .as_deref()
            .unwrap()
            .contains("not named for this mod"),
        "{:?}",
        fluent.download_note
    );
    let plain = by_id("Tests.Plain");
    assert!(plain.download.is_none() && plain.download_note.is_none());
    assert_eq!(
        plain.update_url.as_deref(),
        Some("https://www.nexusmods.com/stardewvalley/mods/1")
    );
    // One repository is read once, however many of its mods have updates.
    assert_eq!(*f.github.reads.lock().unwrap(), 1);
    // A mod with no download says so when asked.
    let error = block_on(f.service.prepare_update(
        &s.profile_id,
        &plain.profile_component_id,
        &installed,
    ))
    .unwrap_err();
    assert_eq!(error.code, "NO_GITHUB_RELEASE");
}

/// A check that cannot reach smapi.io keeps the previous result and says
/// why; nothing is checked for a profile with no mods.
#[test]
fn a_failed_check_keeps_the_last_result() {
    let s = setup();
    let f = fakes(&s);
    let empty = block_on(f.service.check(&s.profile_id, &[], None, None));
    assert!(empty.mods.is_empty() && empty.checked_at.is_some());
    assert!(f.check.asked.lock().unwrap().is_empty(), "nothing was sent");

    install(
        &s,
        &mod_zip(s.tmp.path(), "a.zip", "Tests.Look", "1.0.0", &[]),
    );
    let installed = s
        .state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap();
    *f.check.answers.lock().unwrap() = Some(Ok(vec![update_info("Tests.Look", "1.1.0", None)]));
    let first = block_on(f.service.check(&s.profile_id, &installed, None, None));
    *f.check.answers.lock().unwrap() = Some(Err(AppError::network(
        "MOD_UPDATE_CHECK_UNREACHABLE",
        "The update check at smapi.io could not be reached: offline",
    )));
    let second = block_on(f.service.check(&s.profile_id, &installed, None, None));
    assert!(second.error.as_deref().unwrap().contains("offline"));
    assert_eq!(second.checked_at, first.checked_at);
    assert_eq!(second.mods[0].update_version.as_deref(), Some("1.1.0"));
    // The failure is kept with the result, so reading it again (as the page
    // does when anything changes) still says why.
    let again = f.service.cached(&s.profile_id, &installed);
    assert!(again.error.as_deref().unwrap().contains("offline"));
    // A check that works clears it.
    *f.check.answers.lock().unwrap() = Some(Ok(vec![update_info("Tests.Look", "1.1.0", None)]));
    block_on(f.service.check(&s.profile_id, &installed, None, None));
    assert_eq!(f.service.cached(&s.profile_id, &installed).error, None);
}

/// GitHub being unreachable during the check does not stop the download
/// later; releases read recently are reused by the next check; and a
/// version older than the installed one is not an update.
#[test]
fn github_left_unread_is_read_when_the_update_is_asked_for() {
    let s = setup();
    let f = fakes(&s);
    let published = s.tmp.path().join("published");
    std::fs::create_dir_all(&published).unwrap();
    install(
        &s,
        &mod_zip(s.tmp.path(), "a.zip", "Tests.Kokoro", "2.0", &["Nexus:1"]),
    );
    install(
        &s,
        &mod_zip(
            s.tmp.path(),
            "b.zip",
            "Tests.Beta",
            "1.3.0-beta.1",
            &["Nexus:2"],
        ),
    );
    install(
        &s,
        &mod_zip(s.tmp.path(), "c.zip", "Tests.Nokeys", "1.0.0", &[]),
    );
    let kokoro = mod_zip(&published, "Kokoro.3.0.zip", "Tests.Kokoro", "3.0.0", &[]);
    // Published as "3.0.0" while the check suggests "3.0": the same version.
    publish(&f, "tests/mods", "release/kokoro/3.0.0", &kokoro, None);
    *f.check.answers.lock().unwrap() = Some(Ok(vec![
        update_info("Tests.Kokoro", "3.0", Some("tests/mods")),
        update_info("Tests.Beta", "1.2.0", None),
        ModUpdateInfo {
            unique_id: "Tests.Nokeys".into(),
            ..Default::default()
        },
    ]));
    let installed = s
        .state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap();
    *f.github.unreachable.lock().unwrap() = true;
    let view = block_on(f.service.check(&s.profile_id, &installed, None, None));
    let by_id = |view: &manager_app::api::dto::ModUpdatesDto, id: &str| {
        view.mods
            .iter()
            .find(|m| m.unique_id == id)
            .unwrap()
            .clone()
    };
    let entry = by_id(&view, "Tests.Kokoro");
    assert!(entry.download.is_none() && entry.github_unread);
    assert_eq!(entry.github_repo.as_deref(), Some("tests/mods"));
    assert_eq!(
        entry.download_note.as_deref(),
        Some("GitHub could not be reached.")
    );
    assert_eq!(
        by_id(&view, "Tests.Beta").update_version,
        None,
        "1.2.0 is older"
    );
    assert!(by_id(&view, "Tests.Nokeys").errors[0].contains("no update keys"));

    *f.github.unreachable.lock().unwrap() = false;
    let downloaded = block_on(f.service.prepare_update(
        &s.profile_id,
        &entry.profile_component_id,
        &installed,
    ))
    .unwrap();
    assert_eq!(
        downloaded.source.github().unwrap().asset_name,
        "Kokoro.3.0.zip"
    );
    assert!(downloaded.source.github().unwrap().repo == "tests/mods");

    // Asking again while a review is open closes the earlier one, so only
    // one downloaded update waits for review.
    let again = block_on(f.service.prepare_update(
        &s.profile_id,
        &entry.profile_component_id,
        &installed,
    ))
    .unwrap();
    let first_id = downloaded.preview.operation_id.parse().unwrap();
    let first = s
        .state
        .services
        .operations
        .get_operation(&first_id)
        .unwrap()
        .unwrap();
    assert!(
        !first.state.eq_ignore_ascii_case("draft"),
        "{}",
        first.state
    );
    let second_id = again.preview.operation_id.parse().unwrap();
    s.state
        .services
        .operations
        .cancel_operation(&second_id)
        .unwrap();

    // The download just read the repository, so the next check reuses it.
    let reads = *f.github.reads.lock().unwrap();
    let view = block_on(f.service.check(&s.profile_id, &installed, None, None));
    assert_eq!(*f.github.reads.lock().unwrap(), reads);
    assert!(by_id(&view, "Tests.Kokoro").download.is_some());
}

/// An update for one mod of an archive that held several says, before
/// anything changes, which of the others go with the old archive.
#[test]
fn the_review_names_mods_that_go_with_the_old_archive() {
    let s = setup();
    let f = fakes(&s);
    let published = s.tmp.path().join("published");
    std::fs::create_dir_all(&published).unwrap();
    let bundle = s.tmp.path().join("bundle.zip");
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&bundle).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (folder, id, keys) in [
            ("Core", "Tests.Core", r#"["GitHub:tests/core"]"#),
            ("Extra", "Tests.Extra", "[]"),
        ] {
            zip.start_file(format!("{folder}/manifest.json"), options)
                .unwrap();
            write!(
                zip,
                r#"{{"Name":"{folder}","Author":"Tests","Version":"1.0.0","UniqueID":"{id}","EntryDll":"{folder}.dll","UpdateKeys":{keys}}}"#
            )
            .unwrap();
            zip.start_file(format!("{folder}/{folder}.dll"), options)
                .unwrap();
            write!(zip, "1.0.0").unwrap();
        }
        zip.finish().unwrap();
    }
    install(&s, &bundle);
    let core = mod_zip(&published, "Core.1.1.0.zip", "Tests.Core", "1.1.0", &[]);
    publish(&f, "tests/core", "v1.1.0", &core, None);
    *f.check.answers.lock().unwrap() = Some(Ok(vec![update_info("Tests.Core", "1.1.0", None)]));
    let installed = s
        .state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap();
    let view = block_on(f.service.check(&s.profile_id, &installed, None, None));
    let core = view
        .mods
        .iter()
        .find(|m| m.unique_id == "Tests.Core")
        .unwrap();
    let downloaded = block_on(f.service.prepare_update(
        &s.profile_id,
        &core.profile_component_id,
        &installed,
    ))
    .unwrap();
    assert_eq!(downloaded.preview.replaces.len(), 1);
    assert_eq!(downloaded.preview.replaces[0].also_removed, ["Extra 1.0.0"]);
}

/// A download that is not the size GitHub published is refused and leaves
/// nothing behind.
#[test]
fn a_download_of_the_wrong_size_is_refused() {
    let s = setup();
    let f = fakes(&s);
    let published = s.tmp.path().join("published");
    std::fs::create_dir_all(&published).unwrap();
    install(
        &s,
        &mod_zip(
            s.tmp.path(),
            "a.zip",
            "Tests.Look",
            "1.0.0",
            &["GitHub:tests/look"],
        ),
    );
    let archive = mod_zip(&published, "Look.1.1.0.zip", "Tests.Look", "1.1.0", &[]);
    publish(&f, "tests/look", "v1.1.0", &archive, None);
    f.github
        .releases
        .lock()
        .unwrap()
        .get_mut("tests/look")
        .unwrap()[0]
        .assets[0]
        .size = 10;
    *f.check.answers.lock().unwrap() = Some(Ok(vec![update_info("Tests.Look", "1.1.0", None)]));
    let installed = s
        .state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap();
    let view = block_on(f.service.check(&s.profile_id, &installed, None, None));
    let error = block_on(f.service.prepare_update(
        &s.profile_id,
        &view.mods[0].profile_component_id,
        &installed,
    ))
    .unwrap_err();
    assert_eq!(error.code, "DOWNLOAD_SIZE_MISMATCH");
    let left = std::fs::read_dir(s.state.paths.mod_downloads_dir())
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(left, 0);
}

// --- GitHub over HTTP -------------------------------------------------------

/// A request the fake GitHub received.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    authorization: Option<String>,
}

/// A local HTTP server standing in for GitHub (or for a host it redirects
/// to). `answer` gets the path and the Authorization header and returns the
/// status line, extra headers and body.
struct FakeServer {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

type Answer = dyn Fn(&str, Option<&str>) -> (u16, Vec<(String, String)>, String) + Send + Sync;

impl FakeServer {
    fn start(
        answer: impl Fn(&str, Option<&str>) -> (u16, Vec<(String, String)>, String)
            + Send
            + Sync
            + 'static,
    ) -> Self {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let answer: Arc<Answer> = Arc::new(answer);
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let path = request_line.split(' ').nth(1).unwrap_or("").to_string();
                let mut authorization = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.trim().eq_ignore_ascii_case("authorization") {
                            authorization = Some(value.trim().to_string());
                        }
                    }
                }
                let (status, headers, body) = answer(&path, authorization.as_deref());
                log.lock().unwrap().push(Seen {
                    path,
                    authorization,
                });
                let mut response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str("\r\n");
                response.push_str(&body);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self { base, seen }
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

const TOKEN: &str = "github_pat_11TESTTOKEN_abc123";

/// GitHub's answer listing one release of Kokoro 3.0.0.
fn kokoro_releases() -> String {
    json!([{
        "tag_name": "release/kokoro/3.0.0",
        "name": "Kokoro 3.0.0",
        "draft": false,
        "prerelease": false,
        "html_url": "https://github.com/tests/mods/releases/tag/release/kokoro/3.0.0",
        "assets": [{
            "name": "Kokoro.3.0.zip",
            "size": 10,
            "browser_download_url": "https://github.com/tests/mods/releases/download/release/kokoro/3.0.0/Kokoro.3.0.zip"
        }]
    }])
    .to_string()
}

fn rate_limit_answer(limit: u64) -> String {
    json!({"resources": {"core": {"limit": limit, "remaining": limit - 1, "reset": 1_800_000_000u64}},
           "rate": {"limit": limit, "remaining": limit - 1, "reset": 1_800_000_000u64}})
    .to_string()
}

/// The service reading GitHub over HTTP from `api`, with `token` saved.
fn over_http(s: &Setup, api: &str, token: Option<&str>) -> (Arc<FakeCheck>, ModUpdatesService) {
    let check = Arc::new(FakeCheck::default());
    let tokens = MemoryToken::default();
    if let Some(token) = token {
        tokens.set(token).unwrap();
    }
    let service = ModUpdatesService::new(
        s.state.repo.clone(),
        check.clone(),
        Arc::new(manager_infra::mod_sources::GitHubModReleases::with_api(api)),
        Arc::new(tokens),
        Arc::new(manager_infra::mod_sources::ModDownloads::new(
            Arc::new(manager_infra::http::ReqwestDownloader::new()),
            s.state.paths.mod_downloads_dir(),
        )),
        s.state.services.mods.clone(),
        s.state.services.operations.clone(),
    );
    (check, service)
}

/// Kokoro 2.0 installed, and the check saying 3.0 is out on tests/mods.
fn kokoro_with_update(s: &Setup, check: &FakeCheck) -> Vec<manager_app::api::dto::ModListItemDto> {
    install(
        s,
        &mod_zip(s.tmp.path(), "k.zip", "Tests.Kokoro", "2.0", &["Nexus:1"]),
    );
    *check.answers.lock().unwrap() = Some(Ok(vec![update_info(
        "Tests.Kokoro",
        "3.0",
        Some("tests/mods"),
    )]));
    s.state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap()
}

/// A saved token goes to GitHub's API as `Authorization: Bearer`, with
/// release reads and with the Test button, and is never sent back to the
/// window.
#[test]
fn a_saved_token_is_sent_to_the_github_api() {
    let s = setup();
    let github = FakeServer::start(|path, _| {
        if path == "/rate_limit" {
            (200, vec![], rate_limit_answer(5000))
        } else {
            (200, vec![], kokoro_releases())
        }
    });
    let (check, service) = over_http(&s, &github.base, Some(TOKEN));
    let installed = kokoro_with_update(&s, &check);
    let view = block_on(service.check(&s.profile_id, &installed, None, None));
    assert!(!view.github_token_rejected);
    assert!(view.mods[0].download.is_some(), "{:?}", view.mods[0]);
    let seen = github.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path, "/repos/tests/mods/releases?per_page=30");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(format!("Bearer {TOKEN}").as_str())
    );

    let limit = block_on(service.test_github_token(None)).unwrap();
    assert!(limit.with_token);
    assert_eq!(limit.limit, 5000);
    assert!(limit.resets_at.is_some());
    assert_eq!(
        github.seen()[1].authorization.as_deref(),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    let status = service.github_token_status();
    assert!(status.set && !status.rejected);
    assert!(!serde_json::to_string(&status).unwrap().contains(TOKEN));
    assert!(!serde_json::to_string(&view).unwrap().contains(TOKEN));

    // Without a token nothing is sent.
    service.clear_github_token().unwrap();
    let limit = block_on(service.test_github_token(None)).unwrap();
    assert!(!limit.with_token);
    assert_eq!(github.seen()[2].authorization, None);
}

/// The token never leaves GitHub's API: a redirect to another host drops
/// it, and file downloads never carry it, even from the API's own host.
#[test]
fn the_token_is_not_sent_to_other_hosts() {
    let s = setup();
    let elsewhere = FakeServer::start(|path, _| {
        if path.ends_with(".zip") {
            (200, vec![], "zip".into())
        } else {
            (200, vec![], kokoro_releases())
        }
    });
    let target = elsewhere.base.clone();
    let github = FakeServer::start(move |path, _| {
        (
            302,
            vec![("Location".into(), format!("{target}/moved{path}"))],
            String::new(),
        )
    });
    let (check, service) = over_http(&s, &github.base, Some(TOKEN));
    let installed = kokoro_with_update(&s, &check);
    let view = block_on(service.check(&s.profile_id, &installed, None, None));
    assert!(view.mods[0].download.is_some(), "{:?}", view.mods[0]);
    assert_eq!(
        github.seen()[0].authorization.as_deref(),
        Some(format!("Bearer {TOKEN}").as_str()),
        "the API itself gets it"
    );
    let moved = elsewhere.seen();
    assert_eq!(moved.len(), 1);
    assert!(moved[0].path.starts_with("/moved/repos/"), "{moved:?}");
    assert_eq!(moved[0].authorization, None, "dropped on the redirect");

    // A file download, as the update's download is made: no token, even
    // from the API's host and through its redirect.
    use manager_app::ports::mod_sources::ModDownloadPort;
    let downloads = manager_infra::mod_sources::ModDownloads::new(
        Arc::new(manager_infra::http::ReqwestDownloader::new()),
        s.state.paths.mod_downloads_dir(),
    );
    let file = block_on(downloads.download(
        &format!("{}/download/Kokoro.3.0.zip", github.base),
        None,
        0,
        "Kokoro.3.0.zip",
    ))
    .unwrap();
    downloads.discard(&file);
    assert!(github
        .seen()
        .iter()
        .skip(1)
        .all(|r| r.authorization.is_none()));
    assert!(elsewhere.seen().iter().all(|r| r.authorization.is_none()));
}

/// GitHub refusing the token (401): the read is made again without it, the
/// check says the token was rejected, later reads leave it out, and saving
/// a new one sends that.
#[test]
fn a_rejected_token_falls_back_to_reading_without_it() {
    let s = setup();
    // Release reads after the check's find nothing, so asking for the
    // update reads GitHub and then stops before any download.
    let release_reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let github = FakeServer::start(move |path, authorization| match authorization {
        Some(value) if value != "Bearer github_pat_good" => (
            401,
            vec![],
            json!({"message": "Bad credentials"}).to_string(),
        ),
        _ if path == "/rate_limit" => (200, vec![], rate_limit_answer(60)),
        _ if release_reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 => {
            (200, vec![], kokoro_releases())
        }
        _ => (200, vec![], "[]".into()),
    });
    let (check, service) = over_http(&s, &github.base, Some(TOKEN));
    let installed = kokoro_with_update(&s, &check);
    let view = block_on(service.check(&s.profile_id, &installed, None, None));
    assert!(view.github_token_rejected);
    assert!(view.mods[0].download.is_some(), "read without the token");
    let seen = github.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(seen[0].authorization.is_some());
    assert_eq!(seen[1].authorization, None);
    assert!(service.github_token_status().rejected);
    assert!(
        service
            .cached(&s.profile_id, &installed)
            .github_token_rejected,
        "kept with the result"
    );

    // A download reads the releases again, without the refused token.
    let error = block_on(service.prepare_update(
        &s.profile_id,
        &view.mods[0].profile_component_id,
        &installed,
    ))
    .unwrap_err();
    assert_eq!(error.code, "NO_GITHUB_RELEASE");
    assert_eq!(github.seen()[2].authorization, None);

    // The Test button names the refusal.
    let error = block_on(service.test_github_token(Some(TOKEN))).unwrap_err();
    assert_eq!(error.code, "GITHUB_TOKEN_REJECTED");
    assert!(!format!("{error:?}").contains(TOKEN));

    // A new token is used again.
    service.set_github_token(" github_pat_good ").unwrap();
    assert!(!service.github_token_status().rejected);
    let reads = github.seen().len();
    let _ = block_on(service.prepare_update(
        &s.profile_id,
        &view.mods[0].profile_component_id,
        &installed,
    ));
    assert_eq!(
        github.seen()[reads].authorization.as_deref(),
        Some("Bearer github_pat_good")
    );
    assert!(service.set_github_token("not a token!").is_err());
}

// --- Live ------------------------------------------------------------------

fn try_invoke(
    window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    use tauri::Manager;
    tauri::test::get_ipc_response(
        window,
        tauri::webview::InvokeRequest {
            cmd: cmd.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.into(),
        },
    )
    .map(|value| value.deserialize().unwrap())
    .and_then(|value| stardew_mod_manager::background::answer_of(window.app_handle(), value))
}

/// Real mods, real smapi.io, real GitHub, through the app's commands:
///
/// - Stardew UI 0.6.0, whose manifest has `GitHub:focustense/StardewUI`.
/// - Project Fluent 1.1.0, whose manifest only names Nexus; its repository
///   comes from the compatibility list and holds several mods' releases.
#[test]
#[ignore = "uses the network: smapi.io and GitHub"]
fn live_update_from_github() {
    use manager_infra::http::ReqwestDownloader;
    use tauri::Manager;
    let s = setup();
    let app = stardew_mod_manager::configure(tauri::test::mock_builder(), s.state.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let _ = app.state::<AppState>();

    let downloads = s.tmp.path().join("downloads");
    let downloader = ReqwestDownloader::new();
    for (url, file) in [
        (
            "https://github.com/focustense/StardewUI/releases/download/v0.6.0/StardewUI.0.6.0.zip",
            "StardewUI.0.6.0.zip",
        ),
        (
            "https://github.com/Shockah/Stardew-Valley-Mods/releases/download/release/kokoro/3.0.0/Kokoro.3.0.0.zip",
            "Kokoro.3.0.0.zip",
        ),
        (
            "https://github.com/Shockah/Stardew-Valley-Mods/releases/download/release/project-fluent/1.1.0/ProjectFluent.1.1.0.zip",
            "ProjectFluent.1.1.0.zip",
        ),
    ] {
        let path = block_on(downloader.download_file(url, None, &downloads.join(file))).unwrap();
        install(&s, &path);
    }

    let pid = s.profile_id.to_string();
    let view = try_invoke(&window, "check_mod_updates", json!({ "profileId": pid })).unwrap();
    eprintln!("{}", serde_json::to_string_pretty(&view).unwrap());
    assert_eq!(view["error"], Value::Null);
    let find = |id: &str| {
        view["mods"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["unique_id"] == id)
            .cloned()
            .unwrap()
    };

    for (id, file, declared) in [
        ("focustense.StardewUI", "StardewUI.", true),
        ("Shockah.ProjectFluent", "ProjectFluent.", false),
    ] {
        let entry = find(id);
        let version = entry["update_version"]
            .as_str()
            .expect("an update")
            .to_string();
        assert_eq!(entry["download"]["declared_by_mod"], declared, "{entry}");
        assert!(
            entry["download"]["asset_name"]
                .as_str()
                .unwrap()
                .starts_with(file),
            "{entry}"
        );
        let prepared = try_invoke(
            &window,
            "prepare_mod_update",
            json!({ "profileId": pid, "profileComponentId": entry["profile_component_id"] }),
        )
        .unwrap();
        let preview = &prepared["preview"];
        assert_eq!(preview["replaces"][0]["direction"], "upgrade", "{prepared}");
        assert_eq!(preview["replaces"][0]["incoming_version"], version.as_str());
        try_invoke(
            &window,
            "cancel_active_operation",
            json!({ "operationId": preview["operation_id"] }),
        )
        .unwrap();
        reinstall(&s.state)
            .replace(&s.profile_id, preview["artifact_hash"].as_str().unwrap())
            .unwrap();
        assert_eq!(installed_version(&s, id), version);
        eprintln!(
            "{id} updated to {version} from {}",
            prepared["source"]["release_url"]
        );
    }

    let after = try_invoke(&window, "get_mod_updates", json!({ "profileId": pid })).unwrap();
    for id in ["focustense.StardewUI", "Shockah.ProjectFluent"] {
        let entry = after["mods"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["unique_id"] == id)
            .unwrap();
        assert_eq!(entry["update_version"], Value::Null, "{entry}");
    }
}
