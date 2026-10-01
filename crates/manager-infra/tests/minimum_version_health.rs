//! Enabled mods that declare a minimum SMAPI or game version newer than the
//! installed one are reported, and unknown versions are noted but never
//! treated as too old.

use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::discovery::GameInstallationInspectorPort;
use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, PackageCatalogRepository, ProfileRepository,
    SmapiRepository,
};
use manager_app::services::{HealthService, RuntimeObserver};
use manager_core::deployment::{
    DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
};
use manager_core::game::{
    GameInspection, GameInstallation, ManagementMode, OperatingSystem, Storefront, SupportState,
};
use manager_core::ids::{
    ArtifactHash, DeploymentId, GameInstallationId, ModUniqueId, PackageComponentId,
    ProfileComponentId,
};
use manager_core::manifest::Manifest;
use manager_core::package::{PackageArtifact, PackageComponent};
use manager_core::profile::Profile;
use manager_core::smapi::ManagedSmapiInstallation;
use manager_infra::db::SqliteStateRepository;
use std::path::Path;
use std::sync::Arc;

struct FakeInspector {
    game_version: Option<String>,
}

impl GameInstallationInspectorPort for FakeInspector {
    fn inspect(
        &self,
        path: &Path,
        storefront: Storefront,
        operating_system: OperatingSystem,
    ) -> AppResult<GameInspection> {
        Ok(GameInspection {
            installation_id: None,
            canonical_root: path.to_path_buf(),
            operating_system,
            storefront,
            observed_game_version: self.game_version.clone(),
            observed_smapi_version: None,
            has_existing_smapi: false,
            has_existing_mods: false,
            is_writable: true,
            support_state: SupportState::SupportedManaged,
            evidence: Vec::new(),
            inspected_at: Utc::now(),
        })
    }
}

fn add_mod(
    repo: &SqliteStateRepository,
    profile: &Profile,
    hash_char: char,
    name: &str,
    min_smapi: Option<&str>,
    min_game: Option<&str>,
    enabled: bool,
) {
    let hash = ArtifactHash::parse(hash_char.to_string().repeat(64)).unwrap();
    repo.save_artifact(&PackageArtifact {
        hash: hash.clone(),
        byte_size: 1,
        storage_relative_path: format!("packages/{}.zip", hash.as_str()),
        first_seen_at: Utc::now(),
    })
    .unwrap();
    let manifest = Manifest {
        unique_id: ModUniqueId::new(format!("Author.{name}")),
        name: name.to_string(),
        author: "Author".to_string(),
        version: "1.0.0".to_string(),
        description: None,
        entry_dll: Some("Mod.dll".to_string()),
        minimum_api_version: min_smapi.map(str::to_string),
        minimum_game_version: min_game.map(str::to_string),
        update_keys: Vec::new(),
        dependencies: Vec::new(),
        content_pack_for: None,
    };
    let package_component_id = PackageComponentId::new();
    repo.save_package_component(&PackageComponent {
        id: package_component_id,
        artifact_hash: hash.clone(),
        unique_id: manifest.unique_id.clone(),
        name: manifest.name.clone(),
        author: manifest.author.clone(),
        version: manifest.version.clone(),
        description: None,
        relative_component_root: name.to_string(),
        raw_manifest: "{}".to_string(),
        manifest,
    })
    .unwrap();
    let deployment_id = DeploymentId::new();
    repo.save_deployment(&ProfileDeployment {
        id: deployment_id,
        profile_id: profile.id,
        artifact_hash: hash,
        root_relative_path: name.to_string(),
        installed_at: Utc::now(),
        state: DeploymentState::Present,
    })
    .unwrap();
    repo.save_profile_component(&ProfileComponent {
        id: ProfileComponentId::new(),
        profile_id: profile.id,
        deployment_id,
        package_component_id,
        enabled,
        installed_reason: InstalledReason::Direct,
    })
    .unwrap();
}

struct Fixture {
    repo: Arc<SqliteStateRepository>,
    health: HealthService,
    profile: Profile,
    _tmp: tempfile::TempDir,
}

fn fixture(game_version: Option<&str>) -> Fixture {
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
    let profile = Profile::new(game.id, "Default");
    repo.save_profile(&profile).unwrap();
    repo.save_smapi_installation(&ManagedSmapiInstallation {
        game_installation_id: game.id,
        release_version: "4.1.10".to_string(),
        release_policy_id: "pinned".to_string(),
        installed_at: Utc::now(),
    })
    .unwrap();
    let observer = Arc::new(RuntimeObserver::new(
        repo.clone(),
        repo.clone(),
        repo.clone(),
        Arc::new(FakeInspector {
            game_version: game_version.map(str::to_string),
        }),
    ));
    let health = HealthService::new(
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
    )
    .with_runtime_observer(observer);
    Fixture {
        repo,
        health,
        profile,
        _tmp: tmp,
    }
}

fn minimum_findings(f: &Fixture) -> Vec<(String, Vec<String>)> {
    let mut found: Vec<_> = f
        .health
        .get_health_summary(Some(&f.profile.id))
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.code.starts_with("MOD_NEEDS_NEWER_"))
        .map(|finding| {
            let mut affected = finding.affected_entities;
            affected.sort();
            (finding.code, affected)
        })
        .collect();
    found.sort();
    found
}

#[test]
fn mods_needing_a_newer_runtime_are_named() {
    let f = fixture(Some("1.6.8"));
    add_mod(
        &f.repo,
        &f.profile,
        'a',
        "Fine",
        Some("4.0.0"),
        Some("1.6.0"),
        true,
    );
    add_mod(
        &f.repo,
        &f.profile,
        'b',
        "NewSmapi",
        Some("4.2.0"),
        None,
        true,
    );
    add_mod(
        &f.repo,
        &f.profile,
        'c',
        "NewGame",
        None,
        Some("1.6.15"),
        true,
    );
    add_mod(
        &f.repo,
        &f.profile,
        'd',
        "Disabled",
        Some("9.0.0"),
        Some("9.0"),
        false,
    );
    assert_eq!(
        minimum_findings(&f),
        vec![
            (
                "MOD_NEEDS_NEWER_GAME".to_string(),
                vec!["NewGame".to_string()]
            ),
            (
                "MOD_NEEDS_NEWER_SMAPI".to_string(),
                vec!["NewSmapi".to_string()]
            ),
        ]
    );
}

#[test]
fn an_unknown_game_version_is_noted_not_blamed() {
    let f = fixture(None);
    add_mod(
        &f.repo,
        &f.profile,
        'a',
        "NewGame",
        None,
        Some("1.6.15"),
        true,
    );
    assert_eq!(
        minimum_findings(&f),
        vec![(
            "MOD_NEEDS_NEWER_GAME_UNASSESSED".to_string(),
            vec!["NewGame".to_string()]
        )]
    );
}

#[test]
fn nothing_is_reported_when_every_minimum_is_met() {
    let f = fixture(Some("1.6.15"));
    add_mod(
        &f.repo,
        &f.profile,
        'a',
        "Fine",
        Some("4.1.10"),
        Some("1.6.15"),
        true,
    );
    assert!(minimum_findings(&f).is_empty());
}
