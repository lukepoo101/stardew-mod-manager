//! The command-line interface reads the same state as the app, refuses what
//! it should, and changes nothing without --yes.
#![cfg(target_os = "linux")]

use manager_app::ports::repositories::{GameInstallationRepository, OperationRepository};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{GameInstallationId, OperationId};
use manager_core::operation::{Operation, OperationKind, OperationState};
use manager_infra::paths::AppPaths;
use stardew_mod_manager::cli::{run_with, ATTENTION, INVALID, OK};
use stardew_mod_manager::state::AppState;

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

fn state_with_game() -> (tempfile::TempDir, AppState, GameInstallationId) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let state = AppState::without_startup_recovery(paths).unwrap();
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
    (tmp, state, game_id)
}

#[test]
fn usage_errors_are_invalid() {
    let (_tmp, state, _) = state_with_game();
    assert_eq!(cli(&state, &[]).0, INVALID);
    let (code, _, err) = cli(&state, &["frobnicate"]);
    assert_eq!(code, INVALID);
    assert!(err.contains("Unknown command"));
    assert_eq!(cli(&state, &["mods", "--bogus"]).0, INVALID);
    let (code, out, _) = cli(&state, &["help"]);
    assert_eq!(code, OK);
    assert!(out.contains("never accepts credentials"));
}

#[test]
fn reads_profiles_and_reports_missing_active_profile() {
    let (_tmp, state, game_id) = state_with_game();
    // No active profile yet: a validation refusal, not a crash.
    let (code, _, err) = cli(&state, &["mods"]);
    assert_eq!(code, INVALID);
    assert!(err.contains("No profile is active"));

    let profile = state
        .services
        .profiles
        .create_profile(&game_id, "Cozy", None)
        .unwrap();
    let (code, out, _) = cli(&state, &["profiles", "--json"]);
    assert_eq!(code, OK);
    let list: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(list[0]["name"], "Cozy");

    let (code, out, _) = cli(&state, &["mods", "--profile", &profile.id, "--json"]);
    assert_eq!(code, OK);
    assert_eq!(out.trim(), "[]");

    let (code, _, err) = cli(&state, &["enable", "Some.Mod", "--profile", &profile.id]);
    assert_eq!(code, INVALID);
    assert!(err.contains("No mod with UniqueID 'Some.Mod'"));
}

#[test]
fn an_interrupted_operation_needs_attention_and_blocks_changes() {
    let (_tmp, state, game_id) = state_with_game();
    state
        .repo
        .save_operation(&Operation {
            id: OperationId::new(),
            kind: OperationKind::ModInstall,
            state: OperationState::RecoveryRequired,
            game_installation_id: Some(game_id),
            profile_id: None,
            expected_profile_revision: None,
            plan_schema_version: 1,
            plan_json: "{}".into(),
            progress_current: None,
            progress_total: None,
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: "2026-10-02T00:00:00Z".parse().unwrap(),
            updated_at: "2026-10-02T00:00:00Z".parse().unwrap(),
            completed_at: None,
        })
        .unwrap();
    let (code, out, _) = cli(&state, &["status"]);
    assert_eq!(code, ATTENTION, "{out}");
    assert!(out.contains("Open the app to recover it"));
    // The CLI does not recover it behind the app's back.
    assert!(state
        .services
        .bootstrap
        .get_bootstrap()
        .unwrap()
        .recovery
        .is_some());
    let (code, _, err) = cli(&state, &["disable", "Some.Mod"]);
    assert_eq!(code, ATTENTION);
    assert!(err.contains("needs recovery first"));
}

#[test]
fn json_errors_stay_json_even_for_usage_mistakes() {
    let (_tmp, state, _) = state_with_game();
    for args in [
        &["frob", "--json"][..],
        &["status", "--bogus", "--json"],
        &["mods", "--profile", "--json"],
        &["--json"],
    ] {
        let (code, out, err) = cli(&state, args);
        assert_eq!(code, INVALID, "{args:?}");
        assert!(out.is_empty());
        let error: serde_json::Value = serde_json::from_str(err.trim())
            .unwrap_or_else(|_| panic!("{args:?} printed non-JSON: {err}"));
        assert_eq!(error["category"], "validation");
        assert!(error["summary"].is_string());
    }
}

#[test]
fn readable_output_names_things_and_says_when_there_is_nothing() {
    let (_tmp, state, game_id) = state_with_game();
    let profile = state
        .services
        .profiles
        .create_profile(&game_id, "Cozy", None)
        .unwrap();
    state
        .services
        .profiles
        .switch_active_profile(&game_id, &profile.id.parse().unwrap())
        .unwrap();

    let (code, out, _) = cli(&state, &["status"]);
    assert_eq!(code, OK, "{out}");
    assert!(out.contains(&format!("Active profile: Cozy ({})", profile.id)));
    assert!(out.contains("game"), "the game's folder, not its id: {out}");

    let (code, out, _) = cli(&state, &["mods"]);
    assert_eq!(code, OK);
    assert_eq!(out.trim(), "No mods in this profile.");

    // --profile takes a name as well as an id.
    let (code, out, _) = cli(&state, &["mods", "--profile", "cozy"]);
    assert_eq!(code, OK, "{out}");

    // It does not apply to every command, and says so.
    let (code, _, err) = cli(&state, &["profiles", "--profile", "Cozy"]);
    assert_eq!(code, INVALID);
    assert!(err.contains("--profile does not apply"));

    let (_, usage, _) = cli(&state, &["help"]);
    assert!(usage.contains("health [--profile P]"));
    assert!(usage.contains("help"));
}

#[test]
fn an_unknown_profile_is_refused_not_read_as_empty() {
    let (_tmp, state, _) = state_with_game();
    for profile in ["00000000-0000-0000-0000-000000000000", "Throwaway"] {
        let (code, out, err) = cli(&state, &["mods", "--profile", profile, "--json"]);
        assert_eq!(code, INVALID, "{out}");
        let error: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
        assert_eq!(error["code"], "PROFILE_NOT_FOUND");
        assert!(error["summary"]
            .as_str()
            .unwrap()
            .contains("`cli profiles` lists them"));
    }
}

#[test]
fn archived_profiles_are_listed_and_marked() {
    let (_tmp, state, game_id) = state_with_game();
    let main = state
        .services
        .profiles
        .create_profile(&game_id, "Main", None)
        .unwrap();
    state
        .services
        .profiles
        .switch_active_profile(&game_id, &main.id.parse().unwrap())
        .unwrap();
    let old = state
        .services
        .profiles
        .create_profile(&game_id, "Old", None)
        .unwrap();
    state
        .services
        .profiles
        .archive_profile(&old.id.parse().unwrap())
        .unwrap();
    let (code, out, _) = cli(&state, &["profiles"]);
    assert_eq!(code, OK);
    assert!(
        out.lines()
            .any(|l| l.contains("Old") && l.contains("(archived)")),
        "{out}"
    );
}

#[test]
fn a_fresh_data_folder_does_not_claim_setup_was_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let state = AppState::without_startup_recovery(paths).unwrap();
    let (code, out, _) = cli(&state, &["status", "--json"]);
    assert_eq!(code, OK);
    let status: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["onboarding_disposition"], "not_started");
    assert_eq!(status["app_open"], false);
    assert_eq!(status["set_up"], true);
}

/// Through the real entry point, on a folder the app never set up: status
/// says so, mistakes in the command line are still usage errors, and nothing
/// is created.
#[test]
fn a_never_used_data_folder_is_reported_and_left_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
        let code = stardew_mod_manager::cli::run_at(paths, &args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };

    let (code, out, _) = run(&["status", "--json"]);
    assert_eq!(code, OK);
    let status: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["set_up"], false);

    for args in [&["frob", "--json"][..], &["--json"]] {
        let (code, _, err) = run(args);
        assert_eq!(code, INVALID);
        let error: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
        assert_eq!(error["code"], "CLI_USAGE", "{args:?}");
    }
    let (code, _, err) = run(&["mods"]);
    assert_eq!(code, INVALID);
    assert!(err.contains("NOT_SET_UP"));

    assert_eq!(
        std::fs::read_dir(tmp.path()).unwrap().count(),
        0,
        "reading created nothing"
    );
}
