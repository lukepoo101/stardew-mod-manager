//! Integration coverage for the modern launch verification path.
//!
//! The launch service must be able to prove that a running session actually
//! loaded the profile's mods from a real SMAPI log, and must treat a stopped
//! process as a clean exit only after that proof exists.

use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::deployment::DeploymentPort;
use manager_app::ports::launcher::GameLauncherPort;
use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, LaunchSessionRepository,
    PackageCatalogRepository, ProfileRepository, SmapiRepository,
};
use manager_app::services::LaunchService;
use manager_core::deployment::{
    DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{
    ArtifactHash, DeploymentId, LaunchSessionId, ModUniqueId, PackageComponentId,
    ProfileComponentId,
};
use manager_core::launch::{LaunchMode, LaunchSpec};
use manager_core::manifest::Manifest;
use manager_core::package::{PackageArtifact, PackageComponent};
use manager_core::ports::InstanceLock;
use manager_core::profile::Profile;
use manager_core::smapi::ManagedSmapiInstallation;
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::log_reader::SmapiSessionLogReader;
use manager_infra::paths::AppPaths;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct FakeLauncher {
    running: AtomicBool,
}

struct NoopLock;
impl InstanceLock for NoopLock {
    fn acquire_guard(&self) -> Result<Box<dyn std::any::Any + Send + Sync>, String> {
        Ok(Box::new(()))
    }
}

impl GameLauncherPort for FakeLauncher {
    fn launch_game(&self, _spec: &LaunchSpec) -> AppResult<u32> {
        self.running.store(true, Ordering::SeqCst);
        Ok(4242)
    }

    fn is_game_running(&self, _pid: Option<u32>) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn terminate_game(&self, _pid: Option<u32>) -> AppResult<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

struct Harness {
    service: LaunchService,
    launcher: Arc<FakeLauncher>,
    repo: Arc<SqliteStateRepository>,
    profile_id: manager_core::ids::ProfileId,
    mods_path: std::path::PathBuf,
    log_path: std::path::PathBuf,
    _tmp: tempfile::TempDir,
}

fn add_component(repo: &SqliteStateRepository, profile: &Profile, manifest: Manifest) {
    let hash = ArtifactHash::parse("a".repeat(64)).unwrap();
    repo.save_artifact(&PackageArtifact {
        hash: hash.clone(),
        byte_size: 1,
        storage_relative_path: format!("packages/{}.zip", hash.as_str()),
        first_seen_at: Utc::now(),
    })
    .unwrap();

    let package_component_id = PackageComponentId::new();
    repo.save_package_component(&PackageComponent {
        id: package_component_id,
        artifact_hash: hash.clone(),
        unique_id: manifest.unique_id.clone(),
        name: manifest.name.clone(),
        author: manifest.author.clone(),
        version: manifest.version.clone(),
        description: None,
        relative_component_root: "TestMod".to_string(),
        raw_manifest: "{}".to_string(),
        manifest,
    })
    .unwrap();

    let deployment_id = DeploymentId::new();
    repo.save_deployment(&ProfileDeployment {
        id: deployment_id,
        profile_id: profile.id,
        artifact_hash: hash,
        root_relative_path: "TestMod".to_string(),
        installed_at: Utc::now(),
        state: DeploymentState::Present,
    })
    .unwrap();
    repo.save_profile_component(&ProfileComponent {
        id: ProfileComponentId::new(),
        profile_id: profile.id,
        deployment_id,
        package_component_id,
        enabled: true,
        installed_reason: InstalledReason::Direct,
    })
    .unwrap();
}

fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("state.sqlite3")).unwrap());

    let game = GameInstallation {
        id: manager_core::ids::GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();

    let profile = Profile::new(game.id, "Default");
    repo.save_profile(&profile).unwrap();

    repo.save_smapi_installation(&ManagedSmapiInstallation {
        game_installation_id: game.id,
        release_version: "4.1.10".to_string(),
        release_policy_id: "pinned".to_string(),
        installed_at: Utc::now(),
    })
    .unwrap();

    add_component(
        &repo,
        &profile,
        Manifest {
            unique_id: ModUniqueId::new("Author.TestMod"),
            name: "Test Mod".to_string(),
            author: "Author".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            entry_dll: None,
            minimum_api_version: None,
            minimum_game_version: None,
            update_keys: Vec::new(),
            dependencies: Vec::new(),
            content_pack_for: None,
        },
    );

    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let log_path = tmp.path().join("logs").join("SMAPI-latest.txt");
    std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
    let deployment = Arc::new(FilesystemDeploymentAdapter::new(paths));
    let mods_path = deployment.get_profile_mods_root(&profile.id);

    let launcher = Arc::new(FakeLauncher {
        running: AtomicBool::new(false),
    });
    let service = LaunchService::new(
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        launcher.clone(),
        deployment,
        Arc::new(SmapiSessionLogReader::new(Some(log_path.clone()))),
        Arc::new(NoopLock),
    );

    Harness {
        service,
        launcher,
        repo,
        profile_id: profile.id,
        mods_path,
        log_path,
        _tmp: tmp,
    }
}

fn write_confirming_log(h: &Harness, launch_time: chrono::DateTime<Utc>) {
    let stamp = launch_time.format("%Y-%m-%dT%H:%M:%S UTC").to_string();
    let content = format!(
        "[12:00:00 INFO  SMAPI] Mods go here: {}\n[12:00:00 INFO  SMAPI] Log started at {}\n[12:00:01 TRACE SMAPI] Test Mod (ID: Author.TestMod)\n[12:00:01 INFO  SMAPI] Loaded 1 mods:\n[12:00:01 INFO  SMAPI]    Test Mod 1.0.0 by Author | A test mod\n",
        h.mods_path.display(),
        stamp
    );
    std::fs::write(&h.log_path, content).unwrap();
}

#[test]
fn a_session_whose_mods_appear_in_the_smapi_log_is_confirmed_and_then_exits_cleanly() {
    let h = harness();
    let session = h
        .service
        .launch_profile(&h.profile_id, LaunchMode::Modded)
        .unwrap();
    assert_eq!(session.state, "running_unverified");
    let session_id: LaunchSessionId = session.id.parse().unwrap();

    // The launch baseline must carry the profile's mods directory, otherwise
    // SMAPI's "Mods go here" evidence can never match this session.
    let stored = LaunchSessionRepository::get_launch_session(&*h.repo, &session_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        stored
            .log_baseline
            .as_ref()
            .and_then(|baseline| baseline.expected_mods_path.clone()),
        Some(h.mods_path.clone())
    );
    assert_eq!(stored.expected_mod_ids.len(), 1);

    write_confirming_log(&h, stored.launched_at);

    let polled = h.service.poll_session(&session_id).unwrap().unwrap();
    assert_eq!(polled.state, "mod_load_confirmed");
    assert_eq!(polled.verified_mods, vec!["Author.TestMod".to_string()]);

    h.launcher.terminate_game(None).unwrap();
    let exited = h.service.poll_session(&session_id).unwrap().unwrap();
    assert_eq!(exited.state, "exited");
    assert!(exited.ended_at.is_some());
}

#[test]
fn a_stopped_session_without_log_evidence_is_not_reported_as_a_clean_exit() {
    let h = harness();
    let session = h
        .service
        .launch_profile(&h.profile_id, LaunchMode::Modded)
        .unwrap();
    let session_id: LaunchSessionId = session.id.parse().unwrap();

    h.launcher.terminate_game(None).unwrap();

    let polled = h.service.poll_session(&session_id).unwrap().unwrap();
    assert_eq!(polled.state, "failed");
    assert!(polled.ended_at.is_some());
}
