use manager_app::ports::deployment::DeploymentPort;
use manager_app::ports::repositories::PreferencesRepository;
use manager_core::ids::{OperationId, ProfileId};
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::paths::AppPaths;

#[test]
fn preferences_round_trip_per_key() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = SqliteStateRepository::new(tmp.path().join("state.db")).unwrap();

    assert_eq!(repo.get_preference("theme").unwrap(), None);

    repo.set_preference("theme", "dark").unwrap();
    repo.set_preference("last_profile", "profile-1").unwrap();
    repo.set_preference("theme", "light").unwrap();

    assert_eq!(
        repo.get_preference("theme").unwrap(),
        Some("light".to_string())
    );
    assert_eq!(
        repo.get_preference("last_profile").unwrap(),
        Some("profile-1".to_string())
    );
}

#[test]
fn deployment_paths_cannot_escape_the_profile() {
    let tmp = tempfile::tempdir().unwrap();
    let adapter = FilesystemDeploymentAdapter::new(AppPaths::new(
        tmp.path().join("data"),
        tmp.path().join("cache"),
    ));
    let profile_id = ProfileId::new();
    let operation_id = OperationId::new();

    let staged = tmp.path().join("staged");
    std::fs::create_dir_all(&staged).unwrap();

    for escape in ["../escaped", "/etc/escaped", "nested/../../escaped"] {
        assert!(adapter
            .publish_deployment(&profile_id, &staged, escape)
            .is_err());
        assert!(adapter
            .quarantine_deployment(&profile_id, &operation_id, escape)
            .is_err());
        assert!(adapter
            .restore_quarantined_deployment(&profile_id, &operation_id, escape)
            .is_err());
    }

    assert!(!tmp.path().join("data").join("escaped").exists());
    assert!(adapter
        .publish_deployment(&profile_id, &staged, "Author.Mod")
        .is_ok());
}
