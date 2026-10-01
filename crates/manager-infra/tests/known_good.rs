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
