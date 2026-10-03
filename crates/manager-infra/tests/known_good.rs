//! A known-good record captures the profile's mods and runtime as they were.

use chrono::Utc;
use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, PackageCatalogRepository,
    PreferencesRepository, ProfileRepository,
};
use manager_app::services::{HealthService, KnownGood};
use manager_core::deployment::{
    DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{
    ArtifactHash, DeploymentId, GameInstallationId, ModUniqueId, PackageComponentId,
    ProfileComponentId,
};
use manager_core::launch::RuntimeVersions;
use manager_core::manifest::Manifest;
use manager_core::package::{PackageArtifact, PackageComponent};
use manager_core::profile::Profile;
use manager_infra::db::SqliteStateRepository;
use std::sync::Arc;

#[test]
fn records_mods_and_runtime_and_nothing_before_the_first_success() {
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
    let profile = Profile::new(game.id, "Main");
    repo.save_profile(&profile).unwrap();

    let hash = ArtifactHash::parse("a".repeat(64)).unwrap();
    repo.save_artifact(&PackageArtifact {
        hash: hash.clone(),
        byte_size: 1,
        storage_relative_path: String::new(),
        first_seen_at: Utc::now(),
    })
    .unwrap();
    let deployment = ProfileDeployment {
        id: DeploymentId::new(),
        profile_id: profile.id,
        artifact_hash: hash.clone(),
        root_relative_path: "Mod".into(),
        installed_at: Utc::now(),
        state: DeploymentState::Present,
    };
    repo.save_deployment(&deployment).unwrap();
    let manifest = Manifest {
        unique_id: ModUniqueId::new("A.Mod"),
        name: "A Mod".into(),
        author: "A".into(),
        version: "2.1.0".into(),
        description: None,
        entry_dll: None,
        minimum_api_version: None,
        minimum_game_version: None,
        update_keys: Vec::new(),
        dependencies: Vec::new(),
        content_pack_for: None,
    };
    let pcid = PackageComponentId::new();
    repo.save_package_component(&PackageComponent {
        id: pcid,
        artifact_hash: hash,
        unique_id: manifest.unique_id.clone(),
        name: manifest.name.clone(),
        author: manifest.author.clone(),
        version: manifest.version.clone(),
        description: None,
        relative_component_root: String::new(),
        raw_manifest: "{}".into(),
        manifest,
    })
    .unwrap();
    repo.save_profile_component(&ProfileComponent {
        id: ProfileComponentId::new(),
        profile_id: profile.id,
        deployment_id: deployment.id,
        package_component_id: pcid,
        enabled: false,
        installed_reason: InstalledReason::Direct,
    })
    .unwrap();

    let known_good = KnownGood::new(repo.clone(), repo.clone(), repo.clone());
    assert!(known_good.get(&profile.id).unwrap().is_none());
    known_good
        .record(
            &profile.id,
            &RuntimeVersions {
                game_version: Some("1.6.15".into()),
                smapi_version: Some("4.1.10".into()),
            },
        )
        .unwrap();
    let record = known_good.get(&profile.id).unwrap().unwrap();
    assert_eq!(record.smapi_version.as_deref(), Some("4.1.10"));
    assert_eq!(record.mods.len(), 1);
    assert_eq!(record.mods[0].version, "2.1.0");
    assert!(!record.mods[0].enabled);
    // Without a health check attached, the findings baseline is unknown.
    assert!(record.findings.is_none());
    // Likewise settings, without a way to read them.
    assert!(record.settings.is_none());

    // With one, each settings file's checksum is kept, never its contents.
    struct OneConfig;
    impl manager_app::ports::deployed_files::DeployedFilesPort for OneConfig {
        fn read_folder(
            &self,
            _: &manager_core::ids::ProfileId,
            _: &str,
        ) -> manager_app::error::AppResult<
            Option<Vec<manager_app::ports::deployed_files::DeployedFile>>,
        > {
            Ok(None)
        }
        fn read_configs(
            &self,
            _: &manager_core::ids::ProfileId,
            _: &str,
        ) -> manager_app::error::AppResult<Vec<(String, Vec<u8>)>> {
            Ok(vec![("config.json".into(), b"{}".to_vec())])
        }
        fn write_files(
            &self,
            _: &manager_core::ids::ProfileId,
            _: &str,
            _: &[(String, Vec<u8>)],
        ) -> manager_app::error::AppResult<()> {
            Ok(())
        }
    }
    KnownGood::new(repo.clone(), repo.clone(), repo.clone())
        .with_settings(Arc::new(OneConfig))
        .record(&profile.id, &RuntimeVersions::default())
        .unwrap();
    let settings = known_good
        .get(&profile.id)
        .unwrap()
        .unwrap()
        .settings
        .unwrap();
    assert_eq!(settings.len(), 1);
    assert_eq!(settings[0].path, "config.json");
    assert_eq!(
        settings[0].sha256,
        manager_core::recipe::settings_sha256(b"{}")
    );

    // With one, the findings of that moment are kept (no SMAPI is recorded
    // for this game, so health reports it missing).
    let health = Arc::new(HealthService::new(
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
    ));
    let known_good = KnownGood::new(repo.clone(), repo.clone(), repo.clone()).with_health(health);
    known_good
        .record(&profile.id, &RuntimeVersions::default())
        .unwrap();
    let findings = known_good
        .get(&profile.id)
        .unwrap()
        .unwrap()
        .findings
        .unwrap();
    assert_eq!(
        findings.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(),
        vec!["SMAPI_MISSING"]
    );
    // Recording the same mods again does not add a restore point.
    let points = |repo: &SqliteStateRepository| {
        manager_app::services::restore_points::stored_points(repo, &profile.id).unwrap()
    };
    assert!(points(&repo).is_empty());

    // When the mods change, the setup being replaced is kept as a restore
    // point, so it stays a way back and its packages stay protected.
    let mut enabled = repo.list_profile_components(&profile.id).unwrap().remove(0);
    enabled.enabled = true;
    repo.save_profile_component(&enabled).unwrap();
    known_good
        .record(&profile.id, &RuntimeVersions::default())
        .unwrap();
    let kept = points(&repo);
    assert_eq!(kept.len(), 1);
    assert!(kept[0].label.starts_with("Earlier working setup"));
    assert!(!kept[0].mods[0].enabled);

    // Forgetting removes the record but not that restore point.
    known_good.forget(&profile.id).unwrap();
    assert!(known_good.get(&profile.id).unwrap().is_none());
    assert_eq!(points(&repo).len(), 1);
}

#[test]
fn records_made_before_findings_were_kept_still_read() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("s.sqlite3")).unwrap());
    let profile_id = manager_core::ids::ProfileId::new();
    repo.set_preference(
        &format!("known_good:{profile_id}"),
        r#"{"profile_id":"x","recorded_at":"2026-01-01T00:00:00Z","game_version":null,"smapi_version":null,"mods":[]}"#,
    )
    .unwrap();
    let record = KnownGood::new(repo.clone(), repo.clone(), repo.clone())
        .get(&profile_id)
        .unwrap()
        .unwrap();
    assert!(record.findings.is_none());
}
