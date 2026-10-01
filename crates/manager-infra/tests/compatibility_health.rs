//! Compatibility findings for a profile's enabled mods: minimum SMAPI and
//! game versions, unmet dependencies and content-pack hosts, and UniqueIDs
//! claimed twice. Unknown versions are noted but never treated as too old.

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
use manager_core::manifest::{ContentPackFor, Manifest, ModDependency};
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

fn manifest(id: &str, version: &str) -> Manifest {
    Manifest {
        unique_id: ModUniqueId::new(id),
        name: id.to_string(),
        author: "Author".to_string(),
        version: version.to_string(),
        description: None,
        entry_dll: Some("Mod.dll".to_string()),
        minimum_api_version: None,
        minimum_game_version: None,
        update_keys: Vec::new(),
        dependencies: Vec::new(),
        content_pack_for: None,
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
    let mut manifest = manifest(&format!("Author.{name}"), "1.0.0");
    manifest.name = name.to_string();
    manifest.minimum_api_version = min_smapi.map(str::to_string);
    manifest.minimum_game_version = min_game.map(str::to_string);
    add_manifest(repo, profile, hash_char, name, manifest, enabled);
}

fn add_manifest(
    repo: &SqliteStateRepository,
    profile: &Profile,
    hash_char: char,
    folder: &str,
    manifest: Manifest,
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
    let package_component_id = PackageComponentId::new();
    repo.save_package_component(&PackageComponent {
        id: package_component_id,
        artifact_hash: hash.clone(),
        unique_id: manifest.unique_id.clone(),
        name: manifest.name.clone(),
        author: manifest.author.clone(),
        version: manifest.version.clone(),
        description: None,
        relative_component_root: folder.to_string(),
        raw_manifest: "{}".to_string(),
        manifest,
    })
    .unwrap();
    let deployment_id = DeploymentId::new();
    repo.save_deployment(&ProfileDeployment {
        id: deployment_id,
        profile_id: profile.id,
        artifact_hash: hash,
        root_relative_path: folder.to_string(),
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
    findings_where(f, |code| code.starts_with("MOD_NEEDS_NEWER_"))
}

fn findings_where(f: &Fixture, keep: impl Fn(&str) -> bool) -> Vec<(String, Vec<String>)> {
    let mut found: Vec<_> = f
        .health
        .get_health_summary(Some(&f.profile.id))
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| keep(&finding.code))
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

#[test]
fn unmet_requirements_of_enabled_mods_are_told_apart() {
    let f = fixture(Some("1.6.15"));
    let mut pack = manifest("Me.Pack", "1.0.0");
    pack.content_pack_for = Some(ContentPackFor {
        unique_id: ModUniqueId::new("pathoschild.contentpatcher"),
        minimum_version: Some("2.0.0".to_string()),
    });
    let mut user = manifest("Me.User", "1.0.0");
    user.dependencies.push(ModDependency {
        unique_id: ModUniqueId::new("Me.Library"),
        minimum_version: None,
        is_required: true,
    });
    let mut orphan = manifest("Me.Orphan", "1.0.0");
    orphan.dependencies.push(ModDependency {
        unique_id: ModUniqueId::new("Me.Nowhere"),
        minimum_version: None,
        is_required: true,
    });
    let mut idle = manifest("Me.Idle", "1.0.0");
    idle.dependencies.push(ModDependency {
        unique_id: ModUniqueId::new("Me.Nowhere"),
        minimum_version: None,
        is_required: true,
    });
    add_manifest(
        &f.repo,
        &f.profile,
        'a',
        "CP",
        manifest("Pathoschild.ContentPatcher", "1.30.0"),
        true,
    );
    add_manifest(&f.repo, &f.profile, 'b', "Pack", pack, true);
    add_manifest(
        &f.repo,
        &f.profile,
        'c',
        "Library",
        manifest("Me.Library", "1.0.0"),
        false,
    );
    add_manifest(&f.repo, &f.profile, 'd', "User", user, true);
    add_manifest(&f.repo, &f.profile, 'e', "Orphan", orphan, true);
    // A disabled mod's missing dependency does not matter while it is off.
    add_manifest(&f.repo, &f.profile, 'f', "Idle", idle, false);
    assert_eq!(
        findings_where(&f, |code| code.contains("DEPENDENCY")),
        vec![
            (
                "DEPENDENCY_DISABLED".to_string(),
                vec!["Me.User".to_string()]
            ),
            (
                "DEPENDENCY_TOO_OLD".to_string(),
                vec!["Me.Pack".to_string()]
            ),
            (
                "MISSING_DEPENDENCY".to_string(),
                vec!["Me.Orphan".to_string()]
            ),
        ]
    );
}

#[test]
fn a_unique_id_enabled_twice_is_an_error_naming_both_folders() {
    let f = fixture(Some("1.6.15"));
    add_manifest(
        &f.repo,
        &f.profile,
        'a',
        "Mod",
        manifest("Me.Mod", "1.0.0"),
        true,
    );
    add_manifest(
        &f.repo,
        &f.profile,
        'b',
        "Mod copy",
        manifest("me.mod", "1.1.0"),
        true,
    );
    add_manifest(
        &f.repo,
        &f.profile,
        'c',
        "Mod old",
        manifest("Me.Mod", "0.9.0"),
        false,
    );
    let summary = f.health.get_health_summary(Some(&f.profile.id)).unwrap();
    let duplicates: Vec<_> = summary
        .findings
        .iter()
        .filter(|finding| finding.code == "DUPLICATE_UNIQUE_ID")
        .collect();
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].severity, "error");
    assert_eq!(
        duplicates[0].evidence,
        vec![
            "Me.Mod 1.0.0 in folder 'Mod'".to_string(),
            "me.mod 1.1.0 in folder 'Mod copy'".to_string(),
        ]
    );
}
