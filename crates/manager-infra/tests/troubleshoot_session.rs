//! Enabling and disabling mods moves their files and their recorded state
//! together, is repeatable after an interruption, and names what it affects.

use chrono::Utc;
use manager_app::error::AppResult;
use manager_app::ports::deployment::DeploymentPort;
use manager_app::ports::launcher::{GameLauncherPort, RecordedProcessState};
use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, PackageCatalogRepository, ProfileRepository,
};
use manager_app::services::{ResourceCoordinator, ToggleService, TroubleshootService};
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
    paths: AppPaths,
    service: TroubleshootService,
    profile: Profile,
    /// Profile component ids by mod name.
    ids: std::collections::HashMap<String, ProfileComponentId>,
    _tmp: tempfile::TempDir,
}

fn manifest(id: &str, requires: &[&str]) -> Manifest {
    Manifest {
        unique_id: ModUniqueId::new(id),
        name: id.to_string(),
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

/// `mods` is (name, requires, initially enabled).
fn fixture(mods: &[(&str, &[&str], bool)]) -> Fixture {
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

    let mut ids = std::collections::HashMap::new();
    for (index, (name, requires, enabled)) in mods.iter().enumerate() {
        let hash = ArtifactHash::parse(format!("{:x}", index + 1).repeat(64)).unwrap();
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
            root_relative_path: name.to_string(),
            installed_at: Utc::now(),
            state: DeploymentState::Present,
        };
        repo.save_deployment(&deployment).unwrap();
        let staged = tmp.path().join(format!("staged-{index}"));
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("manifest.json"), "{}").unwrap();
        adapter
            .publish_deployment(&profile.id, &staged, name)
            .unwrap();
        if !enabled {
            adapter.disable_deployment(&profile.id, name).unwrap();
        }
        let m = manifest(name, requires);
        let package_component_id = PackageComponentId::new();
        repo.save_package_component(&PackageComponent {
            id: package_component_id,
            artifact_hash: hash,
            unique_id: m.unique_id.clone(),
            name: m.name.clone(),
            author: m.author.clone(),
            version: m.version.clone(),
            description: None,
            relative_component_root: name.to_string(),
            raw_manifest: "{}".to_string(),
            manifest: m,
        })
        .unwrap();
        let id = ProfileComponentId::new();
        repo.save_profile_component(&ProfileComponent {
            id,
            profile_id: profile.id,
            deployment_id: deployment.id,
            package_component_id,
            enabled: *enabled,
            installed_reason: InstalledReason::Direct,
        })
        .unwrap();
        ids.insert(name.to_string(), id);
    }

    let toggle = Arc::new(ToggleService::new(
        Arc::new(ResourceCoordinator::new()),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        adapter,
        Arc::new(Launcher(AtomicBool::new(false))),
        Arc::new(NoLock),
    ));
    let service = TroubleshootService::new(toggle, repo.clone(), repo.clone(), repo.clone());
    Fixture {
        repo,
        paths,
        service,
        profile,
        ids,
        _tmp: tmp,
    }
}

fn is_enabled(f: &Fixture, name: &str) -> bool {
    let recorded = f
        .repo
        .get_profile_component(&f.ids[name])
        .unwrap()
        .unwrap()
        .enabled;
    let on_disk = f.paths.profile_mods_dir(&f.profile.id).join(name).exists();
    assert_eq!(recorded, on_disk, "{name}: state and files disagree");
    recorded
}

const MODS: &[(&str, &[&str], bool)] = &[
    ("Lib", &[], true),
    ("Alpha", &["Lib"], true),
    ("Beta", &["Lib"], false),
    ("Gamma", &[], true),
    ("Delta", &["Gamma"], true),
    ("Epsilon", &[], true),
];

#[test]
fn the_culprit_is_isolated_and_the_original_setup_is_restored_exactly() {
    // Beta starts disabled, so it can never be the culprit.
    for culprit in ["Lib", "Alpha", "Gamma", "Delta", "Epsilon"] {
        let f = fixture(MODS);
        let state = f.service.start(&f.profile.id).unwrap();
        assert_eq!(state.phase, "all_off");
        assert!(MODS.iter().all(|(name, _, _)| !is_enabled(&f, name)));

        let mut state = state;
        for _ in 0..12 {
            let present = is_enabled(&f, culprit);
            state = f.service.answer(&f.profile.id, present).unwrap();
            assert!(
                !is_enabled(&f, "Beta"),
                "a mod the user turned off stays off"
            );
            if state.phase == "found" || state.phase == "inconclusive" {
                break;
            }
        }
        assert_eq!(state.phase, "found", "culprit {culprit}");
        assert_eq!(state.culprit.as_deref(), Some(culprit));

        // The session is remembered until it is restored.
        assert!(f.service.status(&f.profile.id).unwrap().active);
        let restored = f.service.restore(&f.profile.id).unwrap();
        assert!(!restored.active);
        for (name, _, was_enabled) in MODS {
            assert_eq!(is_enabled(&f, name), *was_enabled, "{name} after restore");
        }
    }
}

#[test]
fn a_problem_that_survives_everything_off_is_not_pinned_on_a_mod() {
    let f = fixture(MODS);
    f.service.start(&f.profile.id).unwrap();
    let state = f.service.answer(&f.profile.id, true).unwrap();
    assert_eq!(state.phase, "inconclusive");
    assert!(state.culprit.is_none());
    f.service.restore(&f.profile.id).unwrap();
    assert!(is_enabled(&f, "Alpha") && !is_enabled(&f, "Beta"));
}

#[test]
fn a_second_session_cannot_start_and_answering_without_one_fails() {
    let f = fixture(MODS);
    assert!(f.service.answer(&f.profile.id, false).is_err());
    f.service.start(&f.profile.id).unwrap();
    assert!(f.service.start(&f.profile.id).is_err());
    f.service.restore(&f.profile.id).unwrap();
    assert!(f.service.start(&f.profile.id).is_ok());
}

#[test]
fn restoring_with_no_session_changes_nothing() {
    let f = fixture(MODS);
    let state = f.service.restore(&f.profile.id).unwrap();
    assert!(!state.active);
    assert!(is_enabled(&f, "Alpha"));
}

#[test]
fn each_answer_is_recorded_and_why_mods_move_together_is_explained() {
    let f = fixture(MODS);
    f.service.start(&f.profile.id).unwrap();
    // The first answer brings mods back; any that need another bring it too.
    let mut state = f.service.answer(&f.profile.id, false).unwrap();
    assert_eq!(state.history.len(), 1);
    assert!(!state.history[0].problem_present);
    assert_eq!(state.history[0].mods_on, 0);
    for _ in 0..12 {
        if state.phase != "testing" {
            break;
        }
        let needs_shown = state
            .together
            .iter()
            .any(|line| line.contains("because it needs them"));
        let pair_on = state.enabled_mods.iter().any(|m| m == "Alpha")
            || state.enabled_mods.iter().any(|m| m == "Delta");
        if pair_on {
            assert!(needs_shown, "{:?}", state.together);
        }
        state = f
            .service
            .answer(
                &f.profile.id,
                state.enabled_mods.iter().any(|m| m == "Delta"),
            )
            .unwrap();
    }
    assert!(state.history.len() >= 2);
    assert!(state.history.windows(2).all(|w| w[0].step < w[1].step));
}

/// Answers truthfully until the session ends: the problem happens when
/// every mod in `together` is on.
fn run(f: &Fixture, together: &[&str]) -> manager_app::api::dto::TroubleshootDto {
    let mut state = f.service.start(&f.profile.id).unwrap();
    for _ in 0..16 {
        let present = together.iter().all(|name| is_enabled(f, name));
        state = f.service.answer(&f.profile.id, present).unwrap();
        if state.phase == "found" || state.phase == "inconclusive" {
            break;
        }
    }
    state
}

#[test]
fn a_problem_needing_two_mods_ends_inconclusive_naming_both() {
    let f = fixture(MODS);
    let state = run(&f, &["Gamma", "Epsilon"]);
    assert_eq!(state.phase, "inconclusive");
    assert!(state.culprit.is_none(), "{:?}", state.culprit);
    let mut named = state.together_with.clone();
    named.extend(state.involved.clone());
    assert!(named.contains(&"Gamma".to_string()), "{named:?}");
    assert!(named.contains(&"Epsilon".to_string()), "{named:?}");
}

#[test]
fn a_found_culprit_is_off_and_everything_else_that_was_on_is_back_on() {
    for (culprit, needs_it) in [("Gamma", &["Delta"][..]), ("Epsilon", &[][..])] {
        let f = fixture(MODS);
        let state = run(&f, &[culprit]);
        assert_eq!(state.phase, "found");
        assert_eq!(state.culprit.as_deref(), Some(culprit));
        assert!(!is_enabled(&f, culprit));
        for (name, _, was_enabled) in MODS {
            if *name == culprit {
                continue;
            }
            let expected = *was_enabled && !needs_it.contains(name);
            assert_eq!(
                is_enabled(&f, name),
                expected,
                "{name} with {culprit} found"
            );
        }
        let held: Vec<&str> = state.held_off.iter().map(String::as_str).collect();
        assert_eq!(held, needs_it);
    }
}

#[test]
fn the_result_is_kept_after_the_session_ends_until_forgotten() {
    let f = fixture(MODS);
    run(&f, &["Epsilon"]);
    let after = f.service.restore(&f.profile.id).unwrap();
    assert!(!after.active);
    let kept = after.last_result.expect("the answer is kept");
    assert_eq!(kept.phase, "found");
    assert_eq!(kept.culprit.as_deref(), Some("Epsilon"));
    assert_eq!(kept.culprit_ids, vec!["Epsilon".to_string()]);
    assert!(f
        .service
        .status(&f.profile.id)
        .unwrap()
        .last_result
        .is_some());
    let forgotten = f.service.forget_result(&f.profile.id).unwrap();
    assert!(forgotten.last_result.is_none());
}

#[test]
fn a_result_found_on_a_copy_is_kept_for_the_profile_it_was_copied_from() {
    use manager_app::services::ProfileExperiments;
    let mut f = fixture(MODS);
    let source = Profile::new(f.profile.game_installation_id, "Main");
    f.repo.save_profile(&source).unwrap();
    let experiments = Arc::new(ProfileExperiments::new(f.repo.clone(), f.repo.clone()));
    experiments.mark(&f.profile.id, &source.id).unwrap();
    let toggle = Arc::new(ToggleService::new(
        Arc::new(ResourceCoordinator::new()),
        f.repo.clone(),
        f.repo.clone(),
        f.repo.clone(),
        f.repo.clone(),
        Arc::new(FilesystemDeploymentAdapter::new(f.paths.clone())),
        Arc::new(Launcher(AtomicBool::new(false))),
        Arc::new(NoLock),
    ));
    f.service = TroubleshootService::new(toggle, f.repo.clone(), f.repo.clone(), f.repo.clone())
        .with_experiments(experiments, f.repo.clone());
    run(&f, &["Gamma"]);
    f.service.restore(&f.profile.id).unwrap();
    let kept = f
        .service
        .status(&source.id)
        .unwrap()
        .last_result
        .expect("kept for the source profile");
    assert_eq!(kept.culprit.as_deref(), Some("Gamma"));
    assert_eq!(kept.tested_in, "Default");
}
