use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::deployment::DeploymentPort;
use manager_app::ports::launcher::{GameLauncherPort, RecordedProcessState};
use manager_app::ports::logging::{ExpectedMod, SessionLogPort};
use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, PackageCatalogRepository, ProfileRepository,
    SmapiRepository,
};
use manager_app::services::LaunchService;
use manager_core::deployment::{
    DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{
    ArtifactHash, DeploymentId, ModUniqueId, PackageComponentId, ProfileComponentId,
};
use manager_core::launch::{
    LaunchMode, LaunchSpec, ProcessIdentity, SessionVerificationBaseline, SessionVerificationResult,
};
use manager_core::manifest::{Manifest, ModDependency};
use manager_core::package::{PackageArtifact, PackageComponent};
use manager_core::ports::InstanceLock;
use manager_core::profile::Profile;
use manager_core::smapi::ManagedSmapiInstallation;
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::paths::AppPaths;
use std::path::PathBuf;
use std::sync::Arc;

struct FakeLauncher;

struct NoopLock;
impl InstanceLock for NoopLock {
    fn acquire_guard(&self) -> Result<Box<dyn std::any::Any + Send + Sync>, String> {
        Ok(Box::new(()))
    }
}

impl GameLauncherPort for FakeLauncher {
    fn launch_game(&self, _spec: &LaunchSpec) -> AppResult<ProcessIdentity> {
        Ok(ProcessIdentity::new(42, Some(1), None))
    }

    fn is_game_running(&self, _pid: Option<u32>) -> bool {
        false
    }

    fn identify_recorded(&self, _identity: &ProcessIdentity) -> RecordedProcessState {
        // Preflight must not report a recorded process as running in this fixture.
        RecordedProcessState::Exited
    }

    fn terminate_game(&self, _pid: Option<u32>) -> AppResult<()> {
        Ok(())
    }
}

struct FakeLog;

impl SessionLogPort for FakeLog {
    fn capture_baseline(&self) -> AppResult<SessionVerificationBaseline> {
        unreachable!("preflight does not read logs")
    }

    fn verify_session(
        &self,
        _baseline: &SessionVerificationBaseline,
        _expected_mods: &[ExpectedMod],
    ) -> AppResult<SessionVerificationResult> {
        unreachable!("preflight does not read logs")
    }

    fn read_log_content(&self) -> AppResult<String> {
        Ok(String::new())
    }

    fn log_file_path(&self) -> PathBuf {
        PathBuf::from("/tmp/SMAPI-latest.txt")
    }

    fn log_is_available(&self) -> bool {
        false
    }
}

fn manifest(
    unique_id: &str,
    version: &str,
    minimum_api_version: Option<&str>,
    dependencies: Vec<ModDependency>,
) -> Manifest {
    Manifest {
        unique_id: ModUniqueId::new(unique_id),
        name: unique_id.to_string(),
        author: "Author".to_string(),
        version: version.to_string(),
        description: None,
        entry_dll: Some("Mod.dll".to_string()),
        minimum_api_version: minimum_api_version.map(str::to_string),
        minimum_game_version: None,
        update_keys: Vec::new(),
        dependencies,
        content_pack_for: None,
    }
}

thread_local! {
    static DATA_DIR: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Creates the folder a recorded deployment refers to, so the fixture matches
/// what an installation leaves behind (launch preflight checks for it).
fn materialize(profile: &Profile, root: &str) {
    DATA_DIR.with(|data| {
        if let Some(data) = data.borrow().as_ref() {
            let dir = AppPaths::new(data.clone(), data.clone())
                .profile_mods_dir(&profile.id)
                .join(root);
            std::fs::create_dir_all(dir).unwrap();
        }
    });
}

fn add_component(
    repo: &SqliteStateRepository,
    profile: &Profile,
    hash_char: char,
    component_manifest: Manifest,
    root: &str,
) {
    let hash = ArtifactHash::parse(hash_char.to_string().repeat(64)).unwrap();
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
        unique_id: component_manifest.unique_id.clone(),
        name: component_manifest.name.clone(),
        author: component_manifest.author.clone(),
        version: component_manifest.version.clone(),
        description: None,
        relative_component_root: root.to_string(),
        raw_manifest: "{}".to_string(),
        manifest: component_manifest,
    })
    .unwrap();

    let deployment_id = DeploymentId::new();
    repo.save_deployment(&ProfileDeployment {
        id: deployment_id,
        profile_id: profile.id,
        artifact_hash: hash,
        root_relative_path: root.to_string(),
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
    materialize(profile, root);
}

fn harness() -> (
    LaunchService,
    Arc<SqliteStateRepository>,
    Profile,
    tempfile::TempDir,
) {
    let tmp = tempfile::tempdir().unwrap();
    DATA_DIR.with(|data| *data.borrow_mut() = Some(tmp.path().join("data")));
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

    let deployment: Arc<dyn DeploymentPort> = Arc::new(FilesystemDeploymentAdapter::new(
        AppPaths::new(tmp.path().join("data"), tmp.path().join("cache")),
    ));
    let service = LaunchService::new(
        Arc::new(manager_app::services::ResourceCoordinator::new()),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        Arc::new(FakeLauncher),
        deployment,
        Arc::new(FakeLog),
        Arc::new(NoopLock),
        Arc::new(manager_infra::TestGameRuntime::for_platform(
            manager_core::game::OperatingSystem::Linux,
        )),
    );
    (service, repo, profile, tmp)
}

#[test]
fn required_dependency_minimum_version_blocks_launch() {
    let (service, repo, profile, _tmp) = harness();
    add_component(
        &repo,
        &profile,
        'a',
        manifest("Author.Framework", "1.0.0", None, Vec::new()),
        "Framework",
    );
    add_component(
        &repo,
        &profile,
        'b',
        manifest(
            "Author.Consumer",
            "1.0.0",
            None,
            vec![ModDependency {
                unique_id: ModUniqueId::new("Author.Framework"),
                minimum_version: Some("2.0.0".to_string()),
                is_required: true,
            }],
        ),
        "Consumer",
    );

    let preflight = service
        .get_launch_preflight(&profile.id, LaunchMode::Modded)
        .unwrap();
    assert!(!preflight.can_launch);
    assert!(preflight
        .blockers
        .iter()
        .any(|message| message.contains("requires version >= 2.0.0")));
}

#[test]
fn satisfied_dependency_version_does_not_block_launch() {
    let (service, repo, profile, _tmp) = harness();
    add_component(
        &repo,
        &profile,
        'd',
        manifest("Author.Framework", "2.1.0", None, Vec::new()),
        "Framework",
    );
    add_component(
        &repo,
        &profile,
        'e',
        manifest(
            "Author.Consumer",
            "1.0.0",
            None,
            vec![ModDependency {
                unique_id: ModUniqueId::new("Author.Framework"),
                minimum_version: Some("2.0.0".to_string()),
                is_required: true,
            }],
        ),
        "Consumer",
    );

    let preflight = service
        .get_launch_preflight(&profile.id, LaunchMode::Modded)
        .unwrap();
    assert!(preflight.blockers.is_empty(), "{:?}", preflight.blockers);
    assert!(preflight.can_launch);
}

#[test]
fn minimum_smapi_version_blocks_launch() {
    let (service, repo, profile, _tmp) = harness();
    add_component(
        &repo,
        &profile,
        'c',
        manifest("Author.FutureMod", "1.0.0", Some("99.0.0"), Vec::new()),
        "FutureMod",
    );

    let preflight = service
        .get_launch_preflight(&profile.id, LaunchMode::Modded)
        .unwrap();
    assert!(!preflight.can_launch);
    assert!(preflight
        .blockers
        .iter()
        .any(|message| message.contains("newer SMAPI")));
}

struct VersionInspector(Option<&'static str>);

impl manager_app::ports::discovery::GameInstallationInspectorPort for VersionInspector {
    fn inspect(
        &self,
        path: &std::path::Path,
        storefront: Storefront,
        operating_system: OperatingSystem,
    ) -> AppResult<manager_core::game::GameInspection> {
        Ok(manager_core::game::GameInspection {
            installation_id: None,
            canonical_root: path.to_path_buf(),
            operating_system,
            storefront,
            observed_game_version: self.0.map(str::to_string),
            observed_smapi_version: None,
            has_existing_smapi: false,
            has_existing_mods: false,
            is_writable: true,
            support_state: manager_core::game::SupportState::SupportedManaged,
            evidence: Vec::new(),
            inspected_at: Utc::now(),
        })
    }
}

fn with_game_version(
    service: LaunchService,
    repo: &Arc<SqliteStateRepository>,
    version: Option<&'static str>,
) -> LaunchService {
    service.with_runtime_observer(Arc::new(manager_app::services::RuntimeObserver::new(
        repo.clone(),
        repo.clone(),
        repo.clone(),
        Arc::new(VersionInspector(version)),
    )))
}

fn needs_game(id: &str, minimum: &str) -> Manifest {
    Manifest {
        minimum_game_version: Some(minimum.to_string()),
        ..manifest(id, "1.0.0", None, Vec::new())
    }
}

/// The warning used to be added to every modded launch, so "Start anyway"
/// was asked for a check that never ran. It is now the result of the check.
#[test]
fn game_version_warnings_come_from_the_mods_minimum_versions() {
    let (service, repo, profile, _tmp) = harness();
    let service = with_game_version(service, &repo, Some("1.6.15"));
    let preflight = service
        .get_launch_preflight(&profile.id, LaunchMode::Modded)
        .unwrap();
    assert!(preflight.warnings.is_empty(), "{:?}", preflight.warnings);

    add_component(&repo, &profile, '1', needs_game("Author.Fine", "1.6.0"), "Fine");
    add_component(&repo, &profile, '2', needs_game("Author.Future", "1.9.0"), "Future");
    let preflight = service
        .get_launch_preflight(&profile.id, LaunchMode::Modded)
        .unwrap();
    assert_eq!(
        preflight.warnings,
        vec!["Author.Future needs Stardew Valley 1.9.0 or newer, and 1.6.15 is in use. SMAPI will probably skip it.".to_string()]
    );
}

#[test]
fn an_unreadable_game_version_is_reported_as_unchecked_only_when_it_matters() {
    let (service, repo, profile, _tmp) = harness();
    let service = with_game_version(service, &repo, None);
    add_component(
        &repo,
        &profile,
        '3',
        manifest("Author.Plain", "1.0.0", None, Vec::new()),
        "Plain",
    );
    assert!(service
        .get_launch_preflight(&profile.id, LaunchMode::Modded)
        .unwrap()
        .warnings
        .is_empty());

    add_component(&repo, &profile, '4', needs_game("Author.Picky", "1.6.0"), "Picky");
    let warnings = service
        .get_launch_preflight(&profile.id, LaunchMode::Modded)
        .unwrap()
        .warnings;
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("The game's version could not be read"));
}

/// Without SMAPI its launcher is missing too; that used to be a second,
/// differently worded blocker pointing at Settings.
#[test]
fn missing_smapi_is_one_blocker_not_two() {
    let (_service, repo, profile, tmp) = harness();
    std::fs::create_dir_all(tmp.path().join("Game")).unwrap();
    repo.delete_smapi_installation(&profile.game_installation_id)
        .unwrap();
    let service = LaunchService::new(
        Arc::new(manager_app::services::ResourceCoordinator::new()),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        Arc::new(FakeLauncher),
        Arc::new(FilesystemDeploymentAdapter::new(AppPaths::new(
            tmp.path().join("data"),
            tmp.path().join("cache"),
        ))),
        Arc::new(FakeLog),
        Arc::new(NoopLock),
        Arc::new(manager_infra::HostGameRuntime::new()),
    );
    let preflight = service
        .get_launch_preflight(&profile.id, LaunchMode::Modded)
        .unwrap();
    assert_eq!(preflight.blockers.len(), 1, "{:?}", preflight.blockers);
    assert!(preflight.blockers[0].starts_with("SMAPI is not installed"));
}
