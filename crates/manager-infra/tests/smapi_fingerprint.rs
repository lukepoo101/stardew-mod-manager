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

/// Writes the files SMAPI's installer leaves in a game folder. Each run
/// writes a different assembly, as a new release would. With `fail` set it
/// writes the files and then reports failure, as an installer that dies
/// partway does.
#[derive(Default)]
struct WritesSmapiFiles {
    runs: std::sync::atomic::AtomicUsize,
    fail: std::sync::atomic::AtomicBool,
}

const LAUNCHER: &str = if cfg!(windows) {
    "StardewModdingAPI.exe"
} else {
    "StardewModdingAPI"
};

impl SmapiInstallerPort for WritesSmapiFiles {
    fn install_smapi(
        &self,
        game_id: &GameInstallationId,
        game_path: &Path,
        _installer_archive: &Path,
        version: &str,
        _expected_sha256: Option<&str>,
    ) -> AppResult<ManagedSmapiInstallation> {
        use std::sync::atomic::Ordering;
        let run = self.runs.fetch_add(1, Ordering::SeqCst);
        let internal = game_path.join("smapi-internal");
        std::fs::create_dir_all(internal.join("i18n")).unwrap();
        for (name, content) in [
            (LAUNCHER, "launcher binary".to_string()),
            (
                "StardewModdingAPI.dll",
                format!("smapi assembly, run {run}"),
            ),
            ("StardewModdingAPI.deps.json", "{}".to_string()),
            ("smapi-internal/SMAPI.Toolkit.dll", "toolkit".to_string()),
            ("smapi-internal/config.json", "{}".to_string()),
            ("smapi-internal/i18n/de.json", "{}".to_string()),
        ] {
            std::fs::write(game_path.join(name), content).unwrap();
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(manager_app::error::AppError::system(
                "SMAPI_INSTALL_FAILED",
                "the installer stopped",
            ));
        }
        Ok(ManagedSmapiInstallation {
            game_installation_id: *game_id,
            release_version: version.to_string(),
            release_policy_id: "catalog".to_string(),
            installed_at: Utc::now(),
        })
    }

    fn uninstall_smapi(&self, game_path: &Path, _: &Path, _: Option<&str>) -> AppResult<()> {
        for name in [
            LAUNCHER,
            "StardewModdingAPI.dll",
            "StardewModdingAPI.deps.json",
        ] {
            let _ = std::fs::remove_file(game_path.join(name));
        }
        let _ = std::fs::remove_dir_all(game_path.join("smapi-internal"));
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
    installer: Arc<WritesSmapiFiles>,
    _tmp: tempfile::TempDir,
}

impl Fixture {
    fn record(&self) -> Option<String> {
        self.repo
            .get_preference(&format!("smapi_fingerprint:{}", self.game_id))
            .unwrap()
    }
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
    let installer = Arc::new(WritesSmapiFiles::default());
    let service = SmapiService::new(
        Arc::new(manager_app::services::ResourceCoordinator::new()),
        repo.clone(),
        repo.clone(),
        Arc::new(ProcessSmapiInstaller::new(paths.smapi_cache_dir())),
        installer.clone(),
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
        installer,
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

/// Review S3: SMAPI rewrites StardewModdingAPI.deps.json from the game's
/// deps.json after every game update, and writes markers at runtime.
#[tokio::test]
async fn files_smapi_rewrites_itself_are_not_changes() {
    let f = fixture();
    f.service.install_smapi(&f.game_id).await.unwrap();
    std::fs::write(
        f.game_root.join("StardewModdingAPI.deps.json"),
        "{\"runtimeTarget\":{\"name\":\"new game build\"}}",
    )
    .unwrap();
    std::fs::write(
        f.game_root
            .join("smapi-internal/StardewModdingAPI.update.marker"),
        "4.2.0",
    )
    .unwrap();
    let status = f.service.get_smapi_status(&f.game_id).unwrap();
    assert!(
        status.changed_files.is_empty(),
        "{:?}",
        status.changed_files
    );
    assert!(!f.record().unwrap().contains("StardewModdingAPI.deps.json"));
}

#[tokio::test]
async fn a_reinstall_records_the_new_files_and_removal_forgets_them() {
    let f = fixture();
    f.service.install_smapi(&f.game_id).await.unwrap();
    let first = f.record().unwrap();
    // Repair, update and roll back all run the installer again.
    f.service.install_smapi(&f.game_id).await.unwrap();
    let second = f.record().unwrap();
    assert_ne!(first, second, "the new assembly is what is recorded");
    assert!(f
        .service
        .get_smapi_status(&f.game_id)
        .unwrap()
        .changed_files
        .is_empty());

    f.service.uninstall_smapi(&f.game_id).await.unwrap();
    assert_eq!(f.record().as_deref(), Some(""));
}

/// Review N3: an install that fails partway (or is completed by recovery)
/// must not leave the old record to report every new file as changed.
#[tokio::test]
async fn a_failed_install_leaves_no_record_rather_than_a_stale_one() {
    let f = fixture();
    f.service.install_smapi(&f.game_id).await.unwrap();
    f.installer
        .fail
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(f.service.install_smapi(&f.game_id).await.is_err());
    assert_eq!(f.record().as_deref(), Some(""));
    assert!(f
        .service
        .get_smapi_status(&f.game_id)
        .unwrap()
        .changed_files
        .is_empty());
}

#[test]
fn a_recorded_path_outside_the_game_folder_is_never_read() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("Game")).unwrap();
    std::fs::write(tmp.path().join("outside.txt"), "x").unwrap();
    let inspector = ProcessSmapiInstaller::new(tmp.path().join("cache"));
    let tampered = |path: &str| manager_core::smapi::SmapiFileFingerprint {
        path: path.to_string(),
        size: 999,
        sha256: "0".repeat(64),
        modified: None,
    };
    let changed = inspector.changed_smapi_files(
        &tmp.path().join("Game"),
        &[
            tampered("../outside.txt"),
            tampered(&tmp.path().join("outside.txt").display().to_string()),
        ],
    );
    assert!(changed.is_empty(), "{changed:?}");
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
