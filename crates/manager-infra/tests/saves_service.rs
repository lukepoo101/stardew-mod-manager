//! Save links follow profile ids, never change saves, and backups are refused
//! while the game runs.

use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::launcher::{GameLauncherPort, RecordedProcessState};
use manager_app::ports::repositories::{GameInstallationRepository, ProfileRepository};
use manager_app::services::SavesService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::GameInstallationId;
use manager_core::launch::{LaunchSpec, ProcessIdentity};
use manager_core::profile::Profile;
use manager_infra::db::SqliteStateRepository;
use manager_infra::saves::FilesystemSaves;
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

#[test]
fn links_survive_renames_and_backups_wait_for_the_game_to_close() {
    let tmp = tempfile::tempdir().unwrap();
    let saves_dir = tmp.path().join("Saves");
    let save = saves_dir.join("Riverside_1");
    std::fs::create_dir_all(&save).unwrap();
    std::fs::write(
        save.join("SaveGameInfo"),
        "<Farmer><name>Sam</name><farmName>Riverside</farmName></Farmer>",
    )
    .unwrap();
    std::fs::write(save.join("Riverside_1"), "world").unwrap();

    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("s.sqlite3")).unwrap());
    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();
    let mut profile = Profile::new(game.id, "Co-op");
    repo.save_profile(&profile).unwrap();
    let launcher = Arc::new(Launcher(AtomicBool::new(false)));
    let service = SavesService::new(
        Arc::new(FilesystemSaves::new(
            Some(saves_dir),
            tmp.path().join("backups"),
        )),
        repo.clone(),
        repo.clone(),
        launcher.clone(),
    );

    service.associate("Riverside_1", Some(&profile.id)).unwrap();
    profile.name = "Co-op with Sam".into();
    repo.save_profile(&profile).unwrap();
    let listed = service.list().unwrap();
    assert_eq!(
        listed.saves[0].profile_name.as_deref(),
        Some("Co-op with Sam")
    );
    assert_eq!(
        std::fs::read_to_string(save.join("Riverside_1")).unwrap(),
        "world"
    );
    assert!(service.associate("Missing_2", Some(&profile.id)).is_err());

    launcher.0.store(true, Ordering::SeqCst);
    assert!(service.backup("Riverside_1").is_err());
    launcher.0.store(false, Ordering::SeqCst);
    let backup = service.backup("Riverside_1").unwrap();
    assert_eq!(service.list().unwrap().saves[0].backups[0].id, backup.id);

    service.associate("Riverside_1", None).unwrap();
    assert!(service.list().unwrap().saves[0].profile_id.is_none());
}
