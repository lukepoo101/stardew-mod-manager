//! Renaming or describing a profile changes only its label.

use chrono::Utc;
use manager_app::ports::repositories::{GameInstallationRepository, ProfileRepository};
use manager_app::services::ProfilesService;
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::GameInstallationId;
use manager_infra::db::SqliteStateRepository;
use std::sync::Arc;

fn setup() -> (
    tempfile::TempDir,
    Arc<SqliteStateRepository>,
    ProfilesService,
    GameInstallationId,
) {
    let tmp = tempfile::tempdir().unwrap();
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
    let service = ProfilesService::new(repo.clone(), repo.clone(), repo.clone());
    (tmp, repo, service, game.id)
}

#[test]
fn renaming_keeps_identity_and_revision() {
    let (_tmp, repo, service, game) = setup();
    let created = service.create_profile(&game, "Co-op", None).unwrap();
    let id = created.id.parse().unwrap();
    // An operation bumps the revision after the rename form was opened.
    let mut profile = repo.get_profile(&id).unwrap().unwrap();
    profile.revision = 7;
    repo.save_profile(&profile).unwrap();

    let renamed = service
        .update_profile_details(&id, "  Co-op with Sam ", Some("  Spring year 3  "))
        .unwrap();
    assert_eq!(renamed.id, created.id);
    assert_eq!(renamed.name, "Co-op with Sam");
    assert_eq!(renamed.description.as_deref(), Some("Spring year 3"));
    let stored = repo.get_profile(&id).unwrap().unwrap();
    assert_eq!(
        stored.revision, 7,
        "a rename must not overwrite a newer revision"
    );
    assert_eq!(stored.created_at, profile.created_at);

    let cleared = service
        .update_profile_details(&id, "Co-op with Sam", Some("   "))
        .unwrap();
    assert_eq!(cleared.description, None);
}

#[test]
fn names_are_validated_and_must_stay_unique() {
    let (_tmp, _repo, service, game) = setup();
    service.create_profile(&game, "Main", None).unwrap();
    let other = service.create_profile(&game, "Other", None).unwrap();
    let id = other.id.parse().unwrap();

    let code = |result: manager_app::error::AppResult<_>| result.unwrap_err().code;
    assert_eq!(
        code(service.update_profile_details(&id, "main", None)),
        "DUPLICATE_PROFILE_NAME"
    );
    assert_eq!(
        code(service.update_profile_details(&id, "  ", None)),
        "EMPTY_PROFILE_NAME"
    );
    assert_eq!(
        code(service.update_profile_details(&id, &"x".repeat(61), None)),
        "PROFILE_NAME_TOO_LONG"
    );
    assert_eq!(
        code(service.update_profile_details(&id, "a\u{7}b", None)),
        "PROFILE_NAME_INVALID"
    );
    // Keeping its own name, in another case, is fine.
    assert_eq!(
        service
            .update_profile_details(&id, "OTHER", None)
            .unwrap()
            .name,
        "OTHER"
    );
    // Creation uses the same rules.
    assert_eq!(
        code(service.create_profile(&game, &"y".repeat(61), None)),
        "PROFILE_NAME_TOO_LONG"
    );
}
