//! Enabling and disabling mods moves their files and their recorded state
//! together, is repeatable after an interruption, and names what it affects.

use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::deployment::DeploymentPort;
use manager_app::ports::launcher::{GameLauncherPort, RecordedProcessState};
use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, PackageCatalogRepository, ProfileRepository,
};
use manager_app::services::{ResourceCoordinator, ToggleService};
use manager_core::deployment::{
    DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{
    ArtifactHash, DeploymentId, ModUniqueId, PackageComponentId, ProfileComponentId,
};
use manager_core::launch::{LaunchSpec, ProcessIdentity};
use manager_core::manifest::{Manifest, ModDependency};
use manager_core::package::{PackageArtifact, PackageComponent};
use manager_core::ports::InstanceLock;
use manager_core::profile::Profile;
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::paths::AppPaths;
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
    adapter: Arc<FilesystemDeploymentAdapter>,
    paths: AppPaths,
    launcher: Arc<Launcher>,
    service: ToggleService,
    profile: Profile,
    /// Two components from one package, and a mod that requires the first.
    bundle: [ProfileComponentId; 2],
    dependent: ProfileComponentId,
    _tmp: tempfile::TempDir,
}

fn manifest(id: &str, name: &str, requires: &[&str]) -> Manifest {
    Manifest {
        unique_id: ModUniqueId::new(id),
        name: name.to_string(),
        author: "A".to_string(),
        version: "1.0.0".to_string(),
        description: None,
        entry_dll: None,
        minimum_api_version: None,
        minimum_game_version: None,
        update_keys: Vec::new(),
        dependencies: requires
            .iter()
            .map(|r| ModDependency {
                unique_id: ModUniqueId::new(*r),
                minimum_version: None,
                is_required: true,
            })
            .collect(),
        content_pack_for: None,
    }
}

fn add(
    repo: &SqliteStateRepository,
    profile: &Profile,
    deployment: &ProfileDeployment,
    manifest: Manifest,
    hash: &ArtifactHash,
) -> ProfileComponentId {
    let package_component_id = PackageComponentId::new();
    repo.save_package_component(&PackageComponent {
        id: package_component_id,
        artifact_hash: hash.clone(),
        unique_id: manifest.unique_id.clone(),
        name: manifest.name.clone(),
        author: manifest.author.clone(),
        version: manifest.version.clone(),
        description: None,
        relative_component_root: manifest.name.clone(),
        raw_manifest: "{}".to_string(),
        manifest,
    })
    .unwrap();
    let id = ProfileComponentId::new();
    repo.save_profile_component(&ProfileComponent {
        id,
        profile_id: profile.id,
        deployment_id: deployment.id,
        package_component_id,
        enabled: true,
        installed_reason: InstalledReason::Direct,
    })
    .unwrap();
    id
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("s.sqlite3")).unwrap());
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let adapter = Arc::new(FilesystemDeploymentAdapter::new(paths.clone()));
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

    let mut ids = Vec::new();
    let mut deployments = Vec::new();
    for (index, rel) in ["Bundle", "Dependent"].iter().enumerate() {
        let hash = ArtifactHash::parse(format!("{index}").repeat(64)).unwrap();
        repo.save_artifact(&PackageArtifact {
            hash: hash.clone(),
            byte_size: 1,
            storage_relative_path: format!("packages/{}.zip", hash.as_str()),
            first_seen_at: Utc::now(),
        })
        .unwrap();
        let deployment = ProfileDeployment {
            id: DeploymentId::new(),
            profile_id: profile.id,
            artifact_hash: hash.clone(),
            root_relative_path: rel.to_string(),
            installed_at: Utc::now(),
            state: DeploymentState::Present,
        };
        repo.save_deployment(&deployment).unwrap();
        let staged = tmp.path().join(format!("staged-{index}"));
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("manifest.json"), "{}").unwrap();
        adapter
            .publish_deployment(&profile.id, &staged, rel)
            .unwrap();
        deployments.push((deployment, hash));
    }
    let (bundle_dep, bundle_hash) = &deployments[0];
    ids.push(add(
        &repo,
        &profile,
        bundle_dep,
        manifest("A.Core", "Core", &[]),
        bundle_hash,
    ));
    ids.push(add(
        &repo,
        &profile,
        bundle_dep,
        manifest("A.Extra", "Extra", &[]),
        bundle_hash,
    ));
    let (dep_dep, dep_hash) = &deployments[1];
    let dependent = add(
        &repo,
        &profile,
        dep_dep,
        manifest("B.Needy", "Needy", &["A.Core"]),
        dep_hash,
    );

    let launcher = Arc::new(Launcher(AtomicBool::new(false)));
    let service = ToggleService::new(
        Arc::new(ResourceCoordinator::new()),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        adapter.clone(),
        launcher.clone(),
        Arc::new(NoLock),
    );
    Fixture {
        repo,
        adapter,
        paths,
        launcher,
        service,
        profile,
        bundle: [ids[0], ids[1]],
        dependent,
        _tmp: tmp,
    }
}

fn enabled(f: &Fixture, id: &ProfileComponentId) -> bool {
    f.repo.get_profile_component(id).unwrap().unwrap().enabled
}

fn in_mods(f: &Fixture, rel: &str) -> bool {
    f.paths.profile_mods_dir(&f.profile.id).join(rel).exists()
}

#[test]
fn disabling_moves_the_whole_package_and_bumps_the_revision() {
    let f = fixture();
    let before = f.repo.get_profile(&f.profile.id).unwrap().unwrap().revision;
    f.service.set_enabled(&f.bundle[0], false).unwrap();

    assert!(!in_mods(&f, "Bundle"));
    assert!(f
        .paths
        .profile_disabled_dir(&f.profile.id)
        .join("Bundle")
        .exists());
    // Both components shared the folder, so both are recorded as disabled.
    assert!(!enabled(&f, &f.bundle[0]) && !enabled(&f, &f.bundle[1]));
    assert!(enabled(&f, &f.dependent) && in_mods(&f, "Dependent"));
    assert_eq!(
        f.repo.get_profile(&f.profile.id).unwrap().unwrap().revision,
        before + 1
    );

    f.service.set_enabled(&f.bundle[1], true).unwrap();
    assert!(in_mods(&f, "Bundle"));
    assert!(enabled(&f, &f.bundle[0]) && enabled(&f, &f.bundle[1]));
}

#[test]
fn repeating_a_toggle_is_harmless_and_does_not_bump_the_revision() {
    let f = fixture();
    f.service.set_enabled(&f.bundle[0], false).unwrap();
    let revision = f.repo.get_profile(&f.profile.id).unwrap().unwrap().revision;
    f.service.set_enabled(&f.bundle[0], false).unwrap();
    assert_eq!(
        f.repo.get_profile(&f.profile.id).unwrap().unwrap().revision,
        revision
    );
}

#[test]
fn a_half_applied_change_is_healed_by_asking_again() {
    let f = fixture();
    // Files moved but the interruption came before the database was updated.
    f.adapter
        .disable_deployment(&f.profile.id, "Bundle")
        .unwrap();
    assert!(enabled(&f, &f.bundle[0]));

    f.service.set_enabled(&f.bundle[0], false).unwrap();
    assert!(!enabled(&f, &f.bundle[0]) && !enabled(&f, &f.bundle[1]));
    assert!(!in_mods(&f, "Bundle"));
}

#[test]
fn nothing_changes_while_the_game_is_running() {
    let f = fixture();
    f.launcher.0.store(true, Ordering::SeqCst);
    assert!(f.service.set_enabled(&f.bundle[0], false).is_err());
    assert!(in_mods(&f, "Bundle") && enabled(&f, &f.bundle[0]));
}

#[test]
fn impact_names_shared_mods_and_the_dependents_that_would_break() {
    let f = fixture();
    let impact = f.service.impact(&f.bundle[0], false).unwrap();
    assert_eq!(impact.affected_mods.len(), 2);
    assert_eq!(impact.dependents, vec!["Needy"]);

    // Enabling the dependent while its requirement is off names the gap.
    f.service.set_enabled(&f.bundle[0], false).unwrap();
    f.service.set_enabled(&f.dependent, false).unwrap();
    let impact = f.service.impact(&f.dependent, true).unwrap();
    assert_eq!(
        impact.unmet_requirements,
        vec!["Needy needs A.Core, which is not enabled"]
    );
    assert!(impact.dependents.is_empty());
}

#[test]
fn an_unknown_mod_is_rejected_without_touching_anything() {
    let f = fixture();
    assert!(f
        .service
        .set_enabled(&ProfileComponentId::new(), false)
        .is_err());
    assert!(in_mods(&f, "Bundle"));
    let _: &dyn DeploymentPort = f.adapter.as_ref();
}

#[test]
fn a_bulk_change_counts_the_selection_as_one_set() {
    let f = fixture();
    // Disabling a mod and the mod that needs it together breaks nothing else.
    let impact = f
        .service
        .impact_many(&[f.bundle[0], f.dependent], false)
        .unwrap();
    assert!(impact.dependents.is_empty());
    assert_eq!(impact.affected_mods.len(), 3);

    let result = f
        .service
        .set_many_enabled(&[f.bundle[0], f.bundle[1], f.dependent], false)
        .unwrap();
    assert_eq!(result.changed, vec!["Core", "Extra", "Needy"]);
    assert!(result.failed.is_empty());
    assert!(!in_mods(&f, "Bundle") && !in_mods(&f, "Dependent"));
    assert!(!enabled(&f, &f.bundle[0]) && !enabled(&f, &f.dependent));

    // Enabling the dependent with its requirement in the same set is fine.
    let impact = f
        .service
        .impact_many(&[f.dependent, f.bundle[1]], true)
        .unwrap();
    assert!(impact.unmet_requirements.is_empty());
    f.service
        .set_many_enabled(&[f.dependent, f.bundle[1]], true)
        .unwrap();
    assert!(in_mods(&f, "Bundle") && in_mods(&f, "Dependent"));
}

#[test]
fn a_bulk_change_needs_a_selection_and_a_stopped_game() {
    let f = fixture();
    assert!(f.service.set_many_enabled(&[], false).is_err());
    f.launcher.0.store(true, Ordering::SeqCst);
    assert!(f
        .service
        .set_many_enabled(&[f.bundle[0], f.dependent], false)
        .is_err());
    assert!(in_mods(&f, "Bundle") && in_mods(&f, "Dependent"));
}
