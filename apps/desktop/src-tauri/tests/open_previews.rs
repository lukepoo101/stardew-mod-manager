//! An install review that is left open never locks the profile.
//!
//! The sequence the testers hit: inspect an archive so "Review mod
//! installation" opens, leave without pressing Cancel, then toggle a mod (in
//! the app or from the command line). The toggle has to work, the abandoned
//! review has to stop looking like work in progress, and confirming it later
//! has to say plainly that it expired.
#![cfg(target_os = "linux")]

use manager_app::ports::repositories::{
    GameInstallationRepository, OperationRepository, SmapiRepository,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{GameInstallationId, OperationId, ProfileComponentId, ProfileId};
use manager_core::operation::{
    AccessMode, Operation, OperationKind, OperationResource, OperationState, ResourceKind,
};
use manager_core::smapi::ManagedSmapiInstallation;
use manager_infra::paths::AppPaths;
use stardew_mod_manager::cli::{run_with, ATTENTION, OK};
use stardew_mod_manager::state::AppState;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

struct Setup {
    tmp: tempfile::TempDir,
    state: AppState,
    profile_id: ProfileId,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let state = AppState::new_with_expected_smapi_hash(paths, Some("test")).unwrap();
    let game_id = GameInstallationId::new();
    state
        .repo
        .save_game(&GameInstallation {
            id: game_id,
            canonical_root: tmp.path().join("game"),
            operating_system: OperatingSystem::Linux,
            storefront: Storefront::Steam,
            management_mode: ManagementMode::Managed,
            created_at: "2026-10-02T00:00:00Z".parse().unwrap(),
        })
        .unwrap();
    state.services.games.set_active_game(&game_id).unwrap();
    state
        .repo
        .save_smapi_installation(&ManagedSmapiInstallation {
            game_installation_id: game_id,
            release_version: "4.1.10".to_string(),
            release_policy_id: "pinned".to_string(),
            installed_at: "2026-10-02T00:00:00Z".parse().unwrap(),
        })
        .unwrap();
    let profile = state
        .services
        .profiles
        .create_profile(&game_id, "Main", None)
        .unwrap();
    let profile_id = ProfileId::from_str(&profile.id).unwrap();
    state
        .services
        .profiles
        .switch_active_profile(&game_id, &profile_id)
        .unwrap();
    Setup {
        tmp,
        state,
        profile_id,
    }
}

fn mod_zip(dir: &Path, id: &str) -> PathBuf {
    mod_zip_requiring(dir, id, None)
}

fn mod_zip_requiring(dir: &Path, id: &str, requires: Option<&str>) -> PathBuf {
    let path = dir.join(format!("{id}.zip"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file(format!("{id}/manifest.json"), options)
        .unwrap();
    let dependencies = requires
        .map(|r| format!(r#","Dependencies":[{{"UniqueID":"Tests.{r}"}}]"#))
        .unwrap_or_default();
    write!(
        zip,
        r#"{{"Name":"{id}","Author":"Tests","Version":"1.0.0","UniqueID":"Tests.{id}","EntryDll":"{id}.dll"{dependencies}}}"#
    )
    .unwrap();
    zip.start_file(format!("{id}/{id}.dll"), options).unwrap();
    zip.write_all(b"fixture").unwrap();
    zip.finish().unwrap();
    path
}

fn install(s: &Setup, id: &str) -> ProfileComponentId {
    install_requiring(s, id, None)
}

fn install_requiring(s: &Setup, id: &str, requires: Option<&str>) -> ProfileComponentId {
    let preview = s
        .state
        .services
        .mods
        .prepare_install(
            &s.profile_id,
            &mod_zip_requiring(s.tmp.path(), id, requires),
        )
        .unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    s.state
        .services
        .operations
        .commit_operation(&OperationId::from_str(&preview.operation_id).unwrap())
        .unwrap();
    let mods = s
        .state
        .mods_queries
        .list_profile_mods(&s.profile_id)
        .unwrap();
    let installed = mods
        .iter()
        .find(|m| m.unique_id == format!("Tests.{id}"))
        .unwrap();
    ProfileComponentId::from_str(&installed.profile_component_id).unwrap()
}

/// Opens a review and walks away from it, as navigating to another page did.
fn abandoned_review(s: &Setup, id: &str) -> OperationId {
    let preview = s
        .state
        .services
        .mods
        .prepare_install(&s.profile_id, &mod_zip(s.tmp.path(), id))
        .unwrap();
    OperationId::from_str(&preview.operation_id).unwrap()
}

fn cli(state: &AppState, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(state, &args, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn state_of(s: &Setup, id: &OperationId) -> Operation {
    s.state.repo.get_operation(id).unwrap().unwrap()
}

#[test]
fn a_toggle_after_leaving_an_install_review_succeeds_and_the_review_expires() {
    let s = setup();
    let installed = install(&s, "Kept");
    let review = abandoned_review(&s, "Left");
    let staging = s.state.paths.profile_staging_dir(&s.profile_id, &review);
    assert!(staging.exists());

    // The toggle the testers could not make.
    s.state
        .services
        .toggle
        .set_enabled(&installed, false)
        .expect("an open review must not block a toggle");

    // The review can never be confirmed now, so it no longer looks open and
    // its staged files are gone.
    let op = state_of(&s, &review);
    assert_eq!(op.state, OperationState::Cancelled);
    assert_eq!(op.error_code.as_deref(), Some("PREVIEW_SUPERSEDED"));
    assert!(!staging.exists());

    // Confirming it after all says what happened, in words a person can act on.
    let expired = s
        .state
        .services
        .operations
        .commit_operation(&review)
        .expect_err("an expired review must not install");
    assert_eq!(expired.code, "PREVIEW_EXPIRED");
    assert_eq!(
        expired.recoverability,
        manager_app::error::Recoverability::RetryWithFreshPlan
    );
    assert!(expired.summary.contains("Nothing was changed"));
    // Closing it again (the review's Cancel) is harmless.
    s.state
        .services
        .operations
        .cancel_operation(&review)
        .unwrap();
}

#[test]
fn the_command_line_reports_an_open_review_and_changes_past_it() {
    let s = setup();
    install(&s, "Kept");
    let review = abandoned_review(&s, "Left");

    let (code, out, _) = cli(&s.state, &["status"]);
    assert_eq!(code, OK, "{out}");
    assert!(out.contains("Open review: Install Left 1.0.0"), "{out}");

    let (code, out, err) = cli(&s.state, &["disable", "Tests.Kept", "--yes"]);
    assert_eq!(code, OK, "{out}{err}");
    assert_eq!(state_of(&s, &review).state, OperationState::Cancelled);

    let (code, out, _) = cli(&s.state, &["status", "--json"]);
    assert_eq!(code, OK);
    let status: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["unfinished_operations"], serde_json::json!([]));
    assert_eq!(status["active_profile_name"], "Main");
}

#[test]
fn a_new_review_does_not_wait_for_an_old_one() {
    let s = setup();
    let first = abandoned_review(&s, "First");
    let second = abandoned_review(&s, "Second");
    // Whichever is confirmed first wins; the other is then stale.
    s.state
        .services
        .operations
        .commit_operation(&second)
        .unwrap();
    assert_eq!(state_of(&s, &first).state, OperationState::Cancelled);
}

/// An unfinished toggle on the profile, as a process carrying it out leaves
/// it in the journal: Committing, with a write claim on the profile.
fn unfinished_toggle(s: &Setup, component: &ProfileComponentId) -> OperationId {
    let id = OperationId::new();
    s.state
        .repo
        .save_operation(&Operation {
            id,
            kind: OperationKind::ModToggle,
            state: OperationState::Committing,
            game_installation_id: None,
            profile_id: Some(s.profile_id),
            expected_profile_revision: None,
            plan_schema_version: 2,
            plan_json: serde_json::json!({
                "enable": false,
                "targets": [{ "folder": "Tests.Kept", "components": [component.to_string()] }],
            })
            .to_string(),
            progress_current: Some(0),
            progress_total: Some(1),
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: "2026-10-02T00:00:00Z".parse().unwrap(),
            updated_at: "2026-10-02T00:00:00Z".parse().unwrap(),
            completed_at: None,
        })
        .unwrap();
    s.state
        .repo
        .save_operation_resource(&OperationResource {
            operation_id: id,
            resource_kind: ResourceKind::Profile,
            resource_id: s.profile_id.to_string(),
            access_mode: AccessMode::Write,
        })
        .unwrap();
    id
}

/// Another process holding the instance lock, as every change does while it
/// moves files. An independent lock object is a separate OS lock holder.
fn another_process_mid_change(s: &Setup) -> Box<dyn std::any::Any + Send + Sync> {
    use manager_core::ports::InstanceLock;
    manager_infra::lock::FileInstanceLock::new(s.state.paths.lock_file_path())
        .acquire_guard()
        .unwrap()
}

fn app_window_open(s: &Setup) -> manager_infra::lock::AppPresenceLock {
    manager_infra::lock::AppPresenceLock::try_hold(stardew_mod_manager::state::presence_lock_path(
        &s.state.paths,
    ))
    .unwrap()
    .unwrap()
}

/// Case 1: another command is mid-change and the app is closed. It is in
/// progress (exit 0), and a change from here waits for it (exit 2).
#[test]
fn another_command_mid_change_with_the_app_closed_is_in_progress() {
    let s = setup();
    let installed = install(&s, "Kept");
    unfinished_toggle(&s, &installed);
    let _holder = another_process_mid_change(&s);

    let (code, out, _) = cli(&s.state, &["status"]);
    assert_eq!(code, OK, "{out}");
    assert!(out.contains("In progress: Turn off Kept"), "{out}");
    assert!(out.contains("App window: closed"), "{out}");

    let (code, _, err) = cli(&s.state, &["disable", "Tests.Kept", "--yes", "--json"]);
    assert_eq!(code, stardew_mod_manager::cli::INVALID, "{err}");
    let error: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(error["code"], "CHANGE_IN_PROGRESS");

    // Recovery never touches it while its owner is alive.
    let _ = s.state.services.operations.retry_recovery();
    let (_, out, _) = cli(&s.state, &["status", "--json"]);
    let status: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["unfinished_operations"][0]["status"], "in_progress");
}

/// Case 2: a command was killed mid-toggle while the app is open. Nothing is
/// carrying it out, so it is interrupted (exit 3), the app refuses changes
/// with a way to recover rather than "wait", and Retry recovery finishes it.
#[test]
fn a_killed_command_with_the_app_open_is_interrupted_and_recoverable() {
    let s = setup();
    let installed = install(&s, "Kept");
    let toggle = unfinished_toggle(&s, &installed);
    let _window = app_window_open(&s);

    let (code, out, _) = cli(&s.state, &["status"]);
    assert_eq!(code, ATTENTION, "{out}");
    assert!(
        out.contains("Interrupted operation: Turn off Kept"),
        "{out}"
    );
    let (code, _, err) = cli(&s.state, &["disable", "Tests.Kept", "--yes", "--json"]);
    assert_eq!(code, ATTENTION, "{err}");
    let error: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(error["code"], "RECOVERY_PENDING");

    // The app's own toggle: not "wait", but recovery.
    let blocked = s
        .state
        .services
        .toggle
        .set_enabled(&installed, false)
        .expect_err("an interrupted change still holds the profile");
    assert_eq!(blocked.code, "PROFILE_OPERATION_UNRESOLVED");
    assert!(
        blocked.summary.contains("turning mods on or off"),
        "{}",
        blocked.summary
    );
    assert_eq!(blocked.operation_id, Some(toggle.to_string()));

    s.state.services.operations.retry_recovery().unwrap();
    assert_eq!(state_of(&s, &toggle).state, OperationState::Succeeded);
    let (code, out, _) = cli(&s.state, &["status"]);
    assert_eq!(code, OK, "{out}");
}

/// Case 3: the app is mid-change. Its own lease on the profile proves it in
/// this process; a change set it drives part by part is alive between parts,
/// in the app and from the command line.
#[test]
fn the_app_mid_change_is_in_progress_and_never_recovered_under_it() {
    let s = setup();
    install(&s, "Kept");
    let change_sets =
        manager_app::services::ChangeSets::new(s.state.repo.clone(), s.state.repo.clone())
            .with_coordinator(s.state.resources.clone());
    let batch = change_sets
        .begin(
            &s.profile_id,
            "Install 2 mods",
            "batch",
            "",
            None,
            &["a.zip".into(), "b.zip".into()],
        )
        .unwrap();
    change_sets.part_done(&batch, 0, None).unwrap();

    // "Check and finish it" between two parts leaves the batch alone.
    s.state.services.operations.retry_recovery().unwrap();
    assert_eq!(state_of(&s, &batch).state, OperationState::Committing);
    let listed = s.state.services.operations.list_operations(None).unwrap();
    assert!(listed
        .iter()
        .any(|op| op.id == batch.to_string() && op.owner_alive));

    // A separate command-line process sees it as in progress while the
    // window is open, and as interrupted once it is not.
    let command_line = AppState::without_startup_recovery(s.state.paths.clone()).unwrap();
    let window = app_window_open(&s);
    let (code, out, _) = cli(&command_line, &["status", "--json"]);
    assert_eq!(code, OK, "{out}");
    let status: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["unfinished_operations"][0]["status"], "in_progress");
    drop(window);
    let (code, _, _) = cli(&command_line, &["status"]);
    assert_eq!(code, ATTENTION);

    change_sets.part_done(&batch, 1, None).unwrap();
    change_sets.finish(&batch, &[]).unwrap();
    assert_eq!(state_of(&s, &batch).state, OperationState::Succeeded);
}

/// Case 4: the app crashed mid-change. No window, no lock: interrupted.
#[test]
fn a_crashed_app_leaves_an_interrupted_change() {
    let s = setup();
    let installed = install(&s, "Kept");
    unfinished_toggle(&s, &installed);
    let (code, out, _) = cli(&s.state, &["status"]);
    assert_eq!(code, ATTENTION, "{out}");
    assert!(out.contains("App window: closed"), "{out}");
    assert!(out.contains("Interrupted operation"), "{out}");
    let (code, _, err) = cli(&s.state, &["disable", "Tests.Kept", "--yes"]);
    assert_eq!(code, ATTENTION, "{err}");
}

/// The command line reviews a change the way the app does before it changes
/// anything, and prints the plan as named objects.
#[test]
fn command_line_plans_warn_about_dependents_like_the_app() {
    let s = setup();
    install(&s, "Core");
    install_requiring(&s, "Addon", Some("Core"));

    let (code, out, _) = cli(&s.state, &["disable", "Tests.Core"]);
    assert_eq!(code, OK);
    assert!(out.contains("Turn off: Tests.Core 1.0.0 (Core)"), "{out}");
    assert!(
        out.contains("will not load while it is off: Addon"),
        "{out}"
    );

    let (code, out, _) = cli(&s.state, &["disable", "Tests.Core", "--json"]);
    assert_eq!(code, OK);
    let plan: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(plan["changes"][0]["unique_id"], "Tests.Core");
    assert_eq!(plan["changes"][0]["name"], "Core");
    assert_eq!(plan["dependents"], serde_json::json!(["Addon"]));
    assert_eq!(plan["applied"], false);

    // Carried out, the warning stays, and the exit code says to look.
    let (code, out, _) = cli(&s.state, &["disable", "Tests.Core", "Tests.Addon", "--yes"]);
    assert_eq!(code, OK, "turning both off leaves nothing broken: {out}");
    let (code, out, _) = cli(&s.state, &["enable", "Tests.Addon", "--yes"]);
    assert_eq!(code, ATTENTION, "{out}");
    assert!(
        out.contains("Addon needs Tests.Core, which is not enabled"),
        "{out}"
    );
    let (_, out, _) = cli(&s.state, &["enable", "Tests.Addon", "--yes"]);
    assert!(out.contains("Already on: Tests.Addon"), "{out}");
}

/// Undoing a removal brings the mod back with the settings it had, which the
/// removal kept in the profile's recovery folder.
#[test]
fn undoing_a_removal_restores_the_mods_settings() {
    let s = setup();
    let installed = install(&s, "Cfg");
    let mods_dir = s.state.paths.profile_mods_dir(&s.profile_id);
    std::fs::write(
        mods_dir.join("Tests.Cfg/config.json"),
        br#"{"Volume": 99, "MyCustomSetting": true}"#,
    )
    .unwrap();

    let removal = s.state.services.mods.prepare_removal(&installed).unwrap();
    let removal = OperationId::from_str(&removal.operation_id).unwrap();
    s.state
        .services
        .operations
        .commit_operation(&removal)
        .unwrap();
    assert!(!mods_dir.join("Tests.Cfg").exists());

    let undo = manager_app::services::removal_undo::RemovalUndo::new(
        s.state.services.operations.clone(),
        manager_app::services::ReinstallService::new(
            s.state.repo.clone(),
            s.state.services.packages.clone(),
            s.state.services.mods.clone(),
            s.state.services.operations.clone(),
            s.state.services.toggle.clone(),
            std::sync::Arc::new(manager_infra::deployed_files::FilesystemDeployedFiles::new(
                s.state.paths.clone(),
            )),
        ),
        std::sync::Arc::new(manager_infra::deployed_files::FilesystemDeployedFiles::new(
            s.state.paths.clone(),
        )),
    );
    let result = undo.undo(&removal).unwrap();
    assert_eq!(result.installed, vec!["Cfg".to_string()]);
    assert_eq!(result.settings_restored, 1);
    assert_eq!(result.settings_problem, None);
    assert_eq!(
        std::fs::read(mods_dir.join("Tests.Cfg/config.json")).unwrap(),
        br#"{"Volume": 99, "MyCustomSetting": true}"#
    );
    assert!(mods_dir.join("Tests.Cfg/Cfg.dll").exists());

    // Only a finished removal can be undone.
    let again = undo.undo(&OperationId::new()).unwrap_err();
    assert_eq!(again.code, "OPERATION_NOT_FOUND");
}
