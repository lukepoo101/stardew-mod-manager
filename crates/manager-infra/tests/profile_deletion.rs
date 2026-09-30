//! Deleting an archived profile removes only that profile's own data.

use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::launcher::{GameLauncherPort, RecordedProcessState};
use manager_app::ports::profile_folders::ProfileFolderPort;
use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, OperationRepository,
    PackageCatalogRepository, ProfileRepository,
};
use manager_app::services::{ProfileDeletionService, ResourceCoordinator};
use manager_core::deployment::{DeploymentState, ProfileDeployment};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{ArtifactHash, DeploymentId, GameInstallationId};
use manager_core::launch::{LaunchSpec, ProcessIdentity};
use manager_core::operation::OperationKind;
use manager_core::package::PackageArtifact;
use manager_core::ports::InstanceLock;
use manager_core::profile::{GameProfileContext, Profile, ProfileState};
use manager_infra::db::SqliteStateRepository;
use manager_infra::paths::AppPaths;
use manager_infra::profile_folders::FilesystemProfileFolders;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct Launcher(AtomicBool);
impl GameLauncherPort for Launcher {
    fn launch_game(&self, _: &LaunchSpec) -> AppResult<ProcessIdentity> {
        unreachable!()
    }
    fn is_game_running(&self, _: Option<u32>) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    fn identify_recorded(&self, _: &ProcessIdentity) -> RecordedProcessState {
        RecordedProcessState::Exited
    }
    fn terminate_game(&self, _: Option<u32>) -> AppResult<()> {
        Ok(())
    }
}

struct NoLock;
impl InstanceLock for NoLock {
    fn acquire_guard(&self) -> Result<Box<dyn std::any::Any + Send + Sync>, String> {
        Ok(Box::new(()))
    }
}

struct Fixture {
    repo: Arc<SqliteStateRepository>,
    paths: AppPaths,
    folders: Arc<FilesystemProfileFolders>,
    launcher: Arc<Launcher>,
    service: ProfileDeletionService,
    game: GameInstallationId,
    _tmp: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("s.sqlite3")).unwrap());
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    paths.ensure_directories().unwrap();
    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();
    let folders = Arc::new(FilesystemProfileFolders::new(paths.clone()));
    let launcher = Arc::new(Launcher(AtomicBool::new(false)));
    let service = ProfileDeletionService::new(
        Arc::new(ResourceCoordinator::new()),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        folders.clone(),
        launcher.clone(),
        Arc::new(NoLock),
    );
    Fixture {
        repo,
        paths,
        folders,
        launcher,
        service,
        game: game.id,
        _tmp: tmp,
    }
}

impl Fixture {
    /// A profile with one mod folder and one deployment of package `c`.
    fn profile(&self, name: &str, state: ProfileState, c: char) -> Profile {
        let mut profile = Profile::new(self.game, name);
        profile.state = state;
        self.repo.save_profile(&profile).unwrap();
        let hash = ArtifactHash::parse(c.to_string().repeat(64)).unwrap();
        self.repo
            .save_artifact(&PackageArtifact {
                hash: hash.clone(),
                byte_size: 1,
                storage_relative_path: String::new(),
                first_seen_at: Utc::now(),
            })
            .unwrap();
        std::fs::write(self.paths.package_path(hash.as_str()), b"zip").unwrap();
        self.repo
            .save_deployment(&ProfileDeployment {
                id: DeploymentId::new(),
                profile_id: profile.id,
                artifact_hash: hash,
                root_relative_path: "Mod".into(),
                installed_at: Utc::now(),
                state: DeploymentState::Present,
            })
            .unwrap();
        let mods = self.paths.profile_mods_dir(&profile.id).join("Mod");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("manifest.json"), b"{}").unwrap();
        profile
    }
}

#[test]
fn deleting_an_archived_profile_removes_only_its_own_data() {
    let f = fixture();
    let keep = f.profile("Keep", ProfileState::Active, 'a');
    let old = f.profile("Old", ProfileState::Archived, 'a');
    f.repo
        .save_game_profile_context(&GameProfileContext {
            game_installation_id: f.game,
            active_profile_id: Some(keep.id),
            default_profile_id: Some(old.id),
            last_active_profile_id: Some(old.id),
        })
        .unwrap();

    let preview = f.service.preview(&old.id).unwrap();
    assert_eq!(preview.name, "Old");
    assert!(preview.folder_bytes > 0);
    assert_eq!(preview.packages_kept, 1);
    assert!(preview.blocked_reason.is_none());

    f.service.delete(&old.id).unwrap();

    assert!(f.repo.get_profile(&old.id).unwrap().is_none());
    assert!(f
        .repo
        .list_deployments_for_profile(&old.id)
        .unwrap()
        .is_empty());
    assert!(!f.paths.profile_dir(&old.id).exists());
    // The folder went to the trash, not away.
    let trashed: Vec<_> = std::fs::read_dir(f.folders.trash_dir()).unwrap().collect();
    assert_eq!(trashed.len(), 1);
    // The other profile, the shared package and the context are consistent.
    assert!(f.paths.profile_mods_dir(&keep.id).join("Mod").exists());
    assert_eq!(
        f.repo.list_deployments_for_profile(&keep.id).unwrap().len(),
        1
    );
    assert!(f.paths.package_path(&"a".repeat(64)).exists());
    let context = f.repo.get_game_profile_context(&f.game).unwrap().unwrap();
    assert_eq!(context.active_profile_id, Some(keep.id));
    assert_eq!(context.default_profile_id, None);
    assert_eq!(context.last_active_profile_id, None);
    // The deletion is in the history.
    let recent = f.repo.list_recent_operations(10).unwrap();
    assert!(recent
        .iter()
        .any(|op| op.kind == OperationKind::ProfileDelete && op.profile_id == Some(old.id)));
}

#[test]
fn only_archived_profiles_can_be_deleted_and_never_while_playing() {
    let f = fixture();
    let active = f.profile("Main", ProfileState::Active, 'b');
    assert!(f
        .service
        .preview(&active.id)
        .unwrap()
        .blocked_reason
        .is_some());
    assert!(f.service.delete(&active.id).is_err());
    assert!(f.paths.profile_dir(&active.id).exists());

    let old = f.profile("Old", ProfileState::Archived, 'c');
    f.launcher.0.store(true, Ordering::SeqCst);
    assert!(f.service.delete(&old.id).is_err());
    assert!(f.repo.get_profile(&old.id).unwrap().is_some());
    assert!(f.paths.profile_dir(&old.id).exists());
}

#[test]
fn only_folders_in_the_trash_can_be_restored() {
    let f = fixture();
    let old = f.profile("Old", ProfileState::Archived, 'd');
    let elsewhere = f._tmp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    assert!(f.folders.restore_from_trash(&old.id, &elsewhere).is_err());
    let trashed = f.folders.move_to_trash(&old.id).unwrap().unwrap();
    assert!(!f.paths.profile_dir(&old.id).exists());
    f.folders.restore_from_trash(&old.id, &trashed).unwrap();
    assert!(f.paths.profile_mods_dir(&old.id).join("Mod").exists());
}
