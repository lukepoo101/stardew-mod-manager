use chrono::Utc;
use manager_app::ports::repositories::{GameInstallationRepository, ProfileRepository};
use manager_app::services::ProfilesService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::profile::{Profile, ProfileState};
use manager_infra::db::SqliteStateRepository;
use std::sync::Arc;

#[test]
fn archived_profiles_cannot_be_reactivated_without_restore() {
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

    let mut archived = Profile::new(game.id, "Archived");
    archived.state = ProfileState::Archived;
    repo.save_profile(&archived).unwrap();

    let service = ProfilesService::new(repo.clone(), repo.clone(), repo.clone());
    let error = service
        .switch_active_profile(&game.id, &archived.id)
        .expect_err("archived profile activation must be rejected");

    assert_eq!(error.code, "PROFILE_ARCHIVED");
}
