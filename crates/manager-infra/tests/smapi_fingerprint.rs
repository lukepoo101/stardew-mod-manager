//! The manager records SMAPI's files when it installs SMAPI, and reports a
//! later change to them: a replaced launcher or a deleted SMAPI library looked
//! exactly like a healthy install, because only presence was checked.

use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::repositories::{GameInstallationRepository, PreferencesRepository};
use manager_app::ports::runtime::{DownloadPort, SmapiInspectorPort, SmapiInstallerPort};
use manager_app::services::SmapiService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::GameInstallationId;
use manager_core::smapi::ManagedSmapiInstallation;
use manager_infra::db::SqliteStateRepository;
use manager_infra::launcher::DetachedGameLauncher;
use manager_infra::lock::FileInstanceLock;
use manager_infra::paths::AppPaths;
use manager_infra::smapi_adapter::ProcessSmapiInstaller;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Writes the files SMAPI's installer leaves in a game folder.
struct WritesSmapiFiles;

impl SmapiInstallerPort for WritesSmapiFiles {
    fn install_smapi(
        &self,
        game_id: &GameInstallationId,
        game_path: &Path,
        _installer_archive: &Path,
        version: &str,
        _expected_sha256: Option<&str>,
    ) -> AppResult<ManagedSmapiInstallation> {
        let internal = game_path.join("smapi-internal");
        std::fs::create_dir_all(internal.join("i18n")).unwrap();
        let launcher = if cfg!(windows) {
            "StardewModdingAPI.exe"
        } else {
            "StardewModdingAPI"
        };
        for (name, content) in [
            (launcher, "launcher binary"),
            ("StardewModdingAPI.dll", "smapi assembly"),
            ("StardewModdingAPI.deps.json", "{}"),
            ("smapi-internal/SMAPI.Toolkit.dll", "toolkit"),
            ("smapi-internal/config.json", "{}"),
            ("smapi-internal/i18n/de.json", "{}"),
        ] {
            std::fs::write(game_path.join(name), content).unwrap();
        }
        Ok(ManagedSmapiInstallation {
            game_installation_id: *game_id,
            release_version: version.to_string(),
            release_policy_id: "catalog".to_string(),
            installed_at: Utc::now(),
        })
    }

    fn uninstall_smapi(&self, _: &Path, _: &Path, _: Option<&str>) -> AppResult<()> {
        Ok(())
    }

    fn archive_sha256(&self, _: &Path) -> AppResult<String> {
        Ok(String::new())
    }

    fn remove_cached_installer(&self, _: &Path) {}
}

struct CachedDownload;

#[async_trait::async_trait]
impl DownloadPort for CachedDownload {
    async fn ensure_downloaded(
        &self,
        _url: &str,
        _expected_sha256: Option<&str>,
        destination: &Path,
    ) -> AppResult<PathBuf> {
        Ok(destination.to_path_buf())
    }

    async fn download_file(
        &self,
        _url: &str,
        _expected_sha256: Option<&str>,
        destination: &Path,
    ) -> AppResult<PathBuf> {
        Ok(destination.to_path_buf())
    }
}

struct Fixture {
    service: SmapiService,
    repo: Arc<SqliteStateRepository>,
    game_id: GameInstallationId,
    game_root: PathBuf,
    _tmp: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("state.sqlite3")).unwrap());
    let game_root = tmp.path().join("Game");
    std::fs::create_dir_all(&game_root).unwrap();
    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: game_root.clone(),
        operating_system: OperatingSystem::host(),
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let service = SmapiService::new(
        Arc::new(manager_app::services::ResourceCoordinator::new()),
        repo.clone(),
        repo.clone(),
        Arc::new(ProcessSmapiInstaller::new(paths.smapi_cache_dir())),
        Arc::new(WritesSmapiFiles),
        Arc::new(CachedDownload),
        paths.smapi_cache_dir(),
        repo.clone(),
        Arc::new(DetachedGameLauncher::isolated()),
        Arc::new(FileInstanceLock::new(paths.lock_file_path())),
    )
    .with_fingerprints(repo.clone());
    Fixture {
        service,
        repo,
        game_id: game.id,
        game_root,
        _tmp: tmp,
    }
}

#[tokio::test]
async fn smapi_files_changed_after_install_are_reported() {
    let f = fixture();
    f.service.install_smapi(&f.game_id).await.unwrap();

    let status = f.service.get_smapi_status(&f.game_id).unwrap();
    assert!(
        status.changed_files.is_empty(),
        "{:?}",
        status.changed_files
    );
    assert!(status
        .evidence
        .iter()
        .any(|line| line == "SMAPI's files match what the manager installed"));
    assert!(
        !status.evidence.iter().any(|line| line.contains("verified")),
        "presence alone is not verification: {:?}",
        status.evidence
    );

    // The launcher replaced by something else, a library deleted, and the
    // user's own SMAPI settings edited.
    let launcher = if cfg!(windows) {
        "StardewModdingAPI.exe"
    } else {
        "StardewModdingAPI"
    };
    std::fs::write(f.game_root.join(launcher), "#!/usr/bin/env python3\n").unwrap();
    std::fs::remove_file(f.game_root.join("smapi-internal/SMAPI.Toolkit.dll")).unwrap();
    std::fs::write(f.game_root.join("smapi-internal/config.user.json"), "{}").unwrap();

    let status = f.service.get_smapi_status(&f.game_id).unwrap();
    assert_eq!(
        status.changed_files,
        vec![
            format!("Changed: {launcher}"),
            "Missing: smapi-internal/SMAPI.Toolkit.dll".to_string(),
        ]
    );
}

#[test]
fn a_same_size_edit_is_still_seen() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("StardewModdingAPI.dll"), "aaaa").unwrap();
    std::fs::create_dir_all(tmp.path().join("smapi-internal")).unwrap();
    let inspector = ProcessSmapiInstaller::new(tmp.path().join("cache"));
    let mut recorded = inspector.fingerprint_smapi(tmp.path());
    assert_eq!(recorded.len(), 1);
    // Forget the time so the contents decide, as when a copy keeps times.
    recorded[0].modified = None;
    std::fs::write(tmp.path().join("StardewModdingAPI.dll"), "bbbb").unwrap();
    assert_eq!(
        inspector.changed_smapi_files(tmp.path(), &recorded),
        vec!["Changed: StardewModdingAPI.dll".to_string()]
    );
}

#[test]
fn without_a_record_nothing_is_claimed_either_way() {
    let f = fixture();
    let status = f.service.get_smapi_status(&f.game_id).unwrap();
    assert!(status.changed_files.is_empty());
    assert!(f
        .repo
        .get_preference(&format!("smapi_fingerprint:{}", f.game_id))
        .unwrap()
        .is_none());
}
