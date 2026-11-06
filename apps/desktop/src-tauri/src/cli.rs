//! Command-line interface for scripted inventory and management.
//!
//! Run as `stardew-mod-manager cli <command>`. Read-only commands report the
//! same state the app shows; the few changes it can make go through the same
//! services, journal and safety checks as the app. It never accepts
//! credentials.
//!
//! Exit codes:
//! - 0: success
//! - 1: the manager could not do it (files, storage, internal error)
//! - 2: the request was invalid or refused (unknown mod, no active profile,
//!   game running, another change in progress)
//! - 3: it worked, but something needs attention: an interrupted operation
//!   waits for recovery, a duplicated profile is unfinished, health has
//!   warnings or errors, or a change left enabled mods without what they need

use crate::state::{presence_lock_path, AppState};
use manager_app::api::dto::{
    BootstrapDto, CliModRefDto, CliOperationStatusDto, CliStatusDto, CliTogglePlanDto,
    ModListItemDto,
};
use manager_app::error::{AppError, AppErrorCategory, AppResult};
use manager_app::ports::repositories::{
    GameInstallationRepository, OperationRepository, ProfileRepository,
};
use manager_core::ids::{GameInstallationId, ProfileComponentId, ProfileId};
use manager_core::operation::OperationState;
use manager_infra::lock::AppPresenceLock;
use manager_infra::paths::AppPaths;
use serde::Serialize;
use std::io::Write;
use std::str::FromStr;

pub const OK: i32 = 0;
pub const FAILED: i32 = 1;
pub const INVALID: i32 = 2;
pub const ATTENTION: i32 = 3;

const COMMANDS: &[&str] = &[
    "help", "status", "profiles", "mods", "health", "enable", "disable",
];

const USAGE: &str = "\
Usage: stardew-mod-manager cli <command> [options]

Read-only:
  status                 Manager version, active game and profile, and any
                         unfinished or interrupted change
  profiles               Profiles for the active game, archived ones included
  mods [--profile P]     Mods in the active (or given) profile
  health [--profile P]   Health findings for the active (or given) profile
  help                   This text

Changes (shows the plan; add --yes to carry it out):
  enable <mod>... [--profile P]   Turn mods on, by UniqueID
  disable <mod>... [--profile P]  Turn mods off, by UniqueID

Options:
  --json                 Machine-readable output (errors too, on stderr)
  --yes                  Carry out a change instead of only showing it
  --profile P            A profile's id or name (see `profiles`)

Exit codes: 0 success, 1 failure, 2 invalid or refused, 3 needs attention.
The CLI never accepts credentials.
";

/// Runs one CLI invocation and returns the process exit code. `args` are the
/// words after `cli`.
pub fn run(args: &[String]) -> i32 {
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let json = args.iter().any(|arg| arg == "--json");
    let paths = match AppPaths::from_env_or_default() {
        Ok(paths) => paths,
        Err(message) => {
            report(
                &mut err,
                json,
                &AppError::new(
                    "DATA_FOLDER_UNAVAILABLE",
                    AppErrorCategory::Storage,
                    format!("Could not open the manager's data: {message}"),
                ),
            );
            return FAILED;
        }
    };
    run_at(paths, args, &mut out, &mut err)
}

/// One invocation against the data folder `paths` describes.
pub fn run_at(paths: AppPaths, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let json = args.iter().any(|arg| arg == "--json");
    // Reading must not set the manager up: on a computer where the app has
    // never run there is nothing to report, and nothing should be created.
    if !paths.state_db_path().exists() {
        return not_set_up(&paths, args, json, out, err);
    }
    let state = match AppState::without_startup_recovery(paths) {
        Ok(state) => state,
        Err(message) => {
            report(
                err,
                json,
                &AppError::new(
                    "DATA_FOLDER_UNAVAILABLE",
                    AppErrorCategory::Storage,
                    format!("Could not open the manager's data: {message}"),
                ),
            );
            return FAILED;
        }
    };
    run_with(&state, args, out, err)
}

/// The answer on a computer where the app has never been set up.
fn not_set_up(
    paths: &AppPaths,
    args: &[String],
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let options = match parse(args) {
        Ok(options) => options,
        Err(message) => return usage_error(err, json, &message),
    };
    // Mistakes in the command line are reported as such, set up or not.
    match options.words.first().map(String::as_str) {
        None => return usage_error(err, json, "Name a command; `help` lists them"),
        Some(command) if !COMMANDS.contains(&command) => {
            return usage_error(err, json, &format!("Unknown command '{command}'."))
        }
        _ => {}
    }
    match options.words.first().map(String::as_str) {
        Some("help") => {
            let _ = write!(out, "{USAGE}");
            OK
        }
        Some("status") => {
            if json {
                emit(
                    out,
                    &serde_json::json!({
                        "set_up": false,
                        "data_dir": paths.data_dir().display().to_string(),
                    }),
                );
            } else {
                let _ = writeln!(
                    out,
                    "Stardew Mod Manager has not been set up here yet (no data in {}). Open the app to set it up.",
                    paths.data_dir().display()
                );
            }
            OK
        }
        _ => {
            report(
                err,
                json,
                &AppError::validation(
                    "NOT_SET_UP",
                    "Stardew Mod Manager has not been set up here yet; open the app to set it up",
                ),
            );
            INVALID
        }
    }
}

struct Options {
    json: bool,
    yes: bool,
    profile: Option<String>,
    words: Vec<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        json: false,
        yes: false,
        profile: None,
        words: Vec::new(),
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" => options.json = true,
            "--yes" => options.yes = true,
            "--profile" => {
                options.profile = Some(
                    iter.next()
                        .filter(|value| !value.starts_with("--"))
                        .ok_or("--profile needs a profile id or name")?
                        .to_string(),
                )
            }
            flag if flag.starts_with("--") => return Err(format!("Unknown option {flag}")),
            word => options.words.push(word.to_string()),
        }
    }
    Ok(options)
}

/// A problem with the command line itself: plain text with the usage, or
/// the same JSON shape as every other error with `--json`.
fn usage_error(err: &mut dyn Write, json: bool, message: &str) -> i32 {
    if json {
        report(err, true, &AppError::validation("CLI_USAGE", message));
    } else {
        let _ = writeln!(err, "{message}\n\n{USAGE}");
    }
    INVALID
}

fn report(err: &mut dyn Write, json: bool, error: &AppError) {
    if json {
        let _ = writeln!(
            err,
            "{}",
            serde_json::json!({
                "code": error.code,
                "category": error.category,
                "summary": error.summary,
            })
        );
    } else {
        let _ = writeln!(err, "{} ({})", error.summary, error.code);
    }
}

/// The testable core: runs against `state`, writing results to `out` and
/// problems to `err`.
pub fn run_with(
    state: &AppState,
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let json = args.iter().any(|arg| arg == "--json");
    let options = match parse(args) {
        Ok(options) => options,
        Err(message) => return usage_error(err, json, &message),
    };
    let Some((command, rest)) = options.words.split_first() else {
        if json {
            return usage_error(err, true, "Name a command; `help` lists them");
        }
        let _ = write!(err, "{USAGE}");
        return INVALID;
    };
    if options.profile.is_some() && matches!(command.as_str(), "status" | "profiles" | "help") {
        return usage_error(
            err,
            json,
            &format!("--profile does not apply to '{command}'"),
        );
    }
    let result = match command.as_str() {
        "help" => {
            let _ = write!(out, "{USAGE}");
            Ok(OK)
        }
        "status" => status(state, &options, out),
        "profiles" => profiles(state, &options, out),
        "mods" => mods(state, &options, out),
        "health" => health(state, &options, out),
        "enable" => toggle(state, &options, rest, true, out),
        "disable" => toggle(state, &options, rest, false, out),
        other => return usage_error(err, json, &format!("Unknown command '{other}'.")),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            report(err, options.json, &error);
            exit_code_for(&error)
        }
    }
}

pub fn exit_code_for(error: &AppError) -> i32 {
    match error.category {
        AppErrorCategory::Validation
        | AppErrorCategory::Dependency
        | AppErrorCategory::OperationConflict
        | AppErrorCategory::Unsupported => INVALID,
        AppErrorCategory::Recovery => ATTENTION,
        _ => FAILED,
    }
}

fn emit<T: Serialize>(out: &mut dyn Write, value: &T) {
    let _ = writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_default()
    );
}

fn bootstrap(state: &AppState) -> AppResult<BootstrapDto> {
    state.services.bootstrap.get_bootstrap()
}

fn active_game(state: &AppState) -> AppResult<GameInstallationId> {
    let game = bootstrap(state)?
        .active_game_installation_id
        .ok_or_else(crate::ipc::no_active_game)?;
    GameInstallationId::from_str(&game).map_err(crate::ipc::invalid_game_installation_id)
}

/// The profile to act on: the one given by id or by name, or the active one.
/// A profile that does not exist is refused rather than read as empty.
fn active_profile(state: &AppState, requested: Option<&str>) -> AppResult<ProfileId> {
    let Some(requested) = requested else {
        let id = bootstrap(state)?
            .active_profile_id
            .ok_or_else(crate::ipc::no_active_profile)?;
        return ProfileId::from_str(&id).map_err(crate::ipc::invalid_profile_id);
    };
    let not_found = || {
        AppError::validation(
            "PROFILE_NOT_FOUND",
            format!("No profile has the id or name '{requested}'. `cli profiles` lists them."),
        )
    };
    if let Ok(id) = ProfileId::from_str(requested) {
        return match state.repo.get_profile(&id)? {
            Some(_) => Ok(id),
            None => Err(not_found()),
        };
    }
    let game = active_game(state)?;
    let mut matches: Vec<_> = all_profiles(state, &game)?
        .into_iter()
        .filter(|profile| profile.name.eq_ignore_ascii_case(requested))
        .collect();
    match matches.len() {
        0 => Err(not_found()),
        1 => ProfileId::from_str(&matches.remove(0).id).map_err(crate::ipc::invalid_profile_id),
        _ => Err(AppError::validation(
            "PROFILE_NAME_AMBIGUOUS",
            format!(
                "More than one profile is called '{requested}'; give its id (see `cli profiles`)"
            ),
        )),
    }
}

fn all_profiles(
    state: &AppState,
    game: &GameInstallationId,
) -> AppResult<Vec<manager_app::api::dto::ProfileSummaryDto>> {
    let mut list = state.services.profiles.list_profiles(game)?;
    list.extend(state.services.profiles.list_archived_profiles(game)?);
    Ok(list)
}

/// Every unfinished change, classified for a script.
///
/// The CLI never recovers anything. What an unfinished change means depends
/// on whether its owner is still carrying it out, which is decided from what
/// the owner holds (see `OperationLiveness`), never from whether a window is
/// open: another command can be mid-change with the app closed, and a killed
/// command leaves its change behind with the app open.
fn unfinished_operations(state: &AppState) -> AppResult<Vec<CliOperationStatusDto>> {
    Ok(state
        .repo
        .list_unresolved_operations()?
        .into_iter()
        .map(|op| {
            let status = if op.state == OperationState::RecoveryRequired {
                "needs_recovery"
            } else if op.state.is_preview() {
                "open_review"
            } else if state.services.operations.owner_alive(&op) {
                "in_progress"
            } else {
                "interrupted"
            };
            CliOperationStatusDto {
                operation_id: op.id.to_string(),
                title: state.services.operations.operation_title(&op),
                status: status.to_string(),
                started_at: op.created_at.to_rfc3339(),
            }
        })
        .collect())
}

fn app_is_open(state: &AppState) -> bool {
    AppPresenceLock::is_held_elsewhere(presence_lock_path(&state.paths))
}

fn status(state: &AppState, options: &Options, out: &mut dyn Write) -> AppResult<i32> {
    let mut boot = bootstrap(state)?;
    // A fresh data folder records setup as completed before any game exists;
    // the app treats that as setup not started, and so does this.
    if boot.onboarding_disposition == "completed" && boot.active_game_installation_id.is_none() {
        boot.onboarding_disposition = "not_started".to_string();
    }
    let app_open = app_is_open(state);
    let game = match boot.active_game_installation_id.as_deref() {
        Some(id) => match GameInstallationId::from_str(id) {
            Ok(id) => state.repo.get_game(&id)?,
            Err(_) => None,
        },
        None => None,
    };
    let profile_name = match boot.active_profile_id.as_deref() {
        Some(id) => match ProfileId::from_str(id) {
            Ok(id) => state.repo.get_profile(&id)?.map(|p| p.name),
            Err(_) => None,
        },
        None => None,
    };
    let unfinished_copies = match &game {
        Some(game) => state
            .services
            .bundle
            .unfinished_copies(&game.id)?
            .into_iter()
            .map(|copy| copy.profile_name)
            .collect(),
        None => Vec::new(),
    };
    let report = CliStatusDto {
        set_up: true,
        bootstrap: boot,
        app_open,
        game_path: game.map(|g| g.canonical_root.display().to_string()),
        active_profile_name: profile_name,
        unfinished_operations: unfinished_operations(state)?,
        unfinished_copies,
    };
    let attention = !report.unfinished_copies.is_empty()
        || report
            .unfinished_operations
            .iter()
            .any(|op| matches!(op.status.as_str(), "needs_recovery" | "interrupted"));

    if options.json {
        emit(out, &report);
    } else {
        let boot = &report.bootstrap;
        let _ = writeln!(out, "Manager version: {}", boot.app_version);
        let _ = writeln!(
            out,
            "Game: {}",
            report.game_path.as_deref().unwrap_or("none chosen")
        );
        let _ = writeln!(
            out,
            "Active profile: {}",
            match (&report.active_profile_name, &boot.active_profile_id) {
                (Some(name), Some(id)) => format!("{name} ({id})"),
                _ => "none".to_string(),
            }
        );
        let _ = writeln!(
            out,
            "App window: {}",
            if report.app_open { "open" } else { "closed" }
        );
        for op in &report.unfinished_operations {
            let line = match op.status.as_str() {
                "needs_recovery" => format!(
                    "Interrupted operation: {} (needs recovery), started {}. Open the app to recover it; changes are refused until then.",
                    op.title, op.started_at
                ),
                "interrupted" => format!(
                    "Interrupted operation: {}, started {}. Whatever was doing it (the app or a command) stopped. Open the app to recover it; changes are refused until then.",
                    op.title, op.started_at
                ),
                "in_progress" => format!(
                    "In progress: {}, started {}. The app or another command is carrying it out; changes to the same profile are refused until it finishes.",
                    op.title, op.started_at
                ),
                _ => format!(
                    "Open review: {}, started {}. Nothing has changed; a change from here cancels it.",
                    op.title, op.started_at
                ),
            };
            let _ = writeln!(out, "{line}");
        }
        for name in &report.unfinished_copies {
            let _ = writeln!(
                out,
                "Unfinished copy: {name}. Open the app, Profiles, Finish copying."
            );
        }
        if report.unfinished_operations.is_empty() && report.unfinished_copies.is_empty() {
            let _ = writeln!(out, "No interrupted operations.");
        }
    }
    Ok(if attention { ATTENTION } else { OK })
}

fn profiles(state: &AppState, options: &Options, out: &mut dyn Write) -> AppResult<i32> {
    let boot = bootstrap(state)?;
    let game = active_game(state)?;
    let list = all_profiles(state, &game)?;
    if options.json {
        emit(out, &list);
    } else {
        for profile in &list {
            let active = boot.active_profile_id.as_deref() == Some(profile.id.as_str());
            let _ = writeln!(
                out,
                "{}{}\t{}\t{} mod(s){}",
                if active { "* " } else { "  " },
                profile.id,
                profile.name,
                profile.mod_count,
                if profile.state == "archived" {
                    "\t(archived)"
                } else {
                    ""
                }
            );
        }
    }
    Ok(OK)
}

fn list_mods(state: &AppState, profile: &ProfileId) -> AppResult<Vec<ModListItemDto>> {
    state.mods_queries.list_profile_mods(profile)
}

fn mods(state: &AppState, options: &Options, out: &mut dyn Write) -> AppResult<i32> {
    let profile = active_profile(state, options.profile.as_deref())?;
    let list = list_mods(state, &profile)?;
    if options.json {
        emit(out, &list);
    } else if list.is_empty() {
        let _ = writeln!(out, "No mods in this profile.");
    } else {
        for item in &list {
            let _ = writeln!(
                out,
                "{}\t{}\t{}\t{}",
                if item.enabled { "on " } else { "off" },
                item.unique_id,
                item.version,
                item.name
            );
        }
    }
    Ok(OK)
}

fn health(state: &AppState, options: &Options, out: &mut dyn Write) -> AppResult<i32> {
    let profile = active_profile(state, options.profile.as_deref())?;
    let overview = state.profile_queries.get_profile_overview(&profile)?;
    let summary = overview.health_summary;
    if options.json {
        emit(out, &summary);
    } else {
        let _ = writeln!(
            out,
            "Health: {} ({} error(s), {} warning(s), {} note(s))",
            summary.status, summary.error_count, summary.warning_count, summary.info_count
        );
        for finding in &summary.findings {
            let _ = writeln!(
                out,
                "[{}] {}: {}\n    {}",
                finding.severity, finding.code, finding.title, finding.summary
            );
        }
    }
    Ok(if summary.error_count + summary.warning_count > 0 {
        ATTENTION
    } else {
        OK
    })
}

fn mod_ref(item: &ModListItemDto) -> CliModRefDto {
    CliModRefDto {
        unique_id: item.unique_id.clone(),
        name: item.name.clone(),
        version: item.version.clone(),
    }
}

fn toggle(
    state: &AppState,
    options: &Options,
    requested: &[String],
    enable: bool,
    out: &mut dyn Write,
) -> AppResult<i32> {
    if requested.is_empty() {
        return Err(AppError::new(
            "CLI_NO_MODS_GIVEN",
            AppErrorCategory::Validation,
            "Name at least one mod by its UniqueID",
        ));
    }
    // An operation the app stopped in the middle of is only recovered by the
    // app; until then the files may not match the records.
    if let Some(op) = unfinished_operations(state)?
        .into_iter()
        .find(|op| matches!(op.status.as_str(), "needs_recovery" | "interrupted"))
    {
        return Err(AppError::new(
            "RECOVERY_PENDING",
            AppErrorCategory::Recovery,
            format!(
                "An interrupted operation ({}) needs recovery first; open the app to recover it",
                op.title
            ),
        ));
    }
    let profile = active_profile(state, options.profile.as_deref())?;
    let installed = list_mods(state, &profile)?;
    let mut ids = Vec::new();
    let mut plan = CliTogglePlanDto {
        enable,
        changes: Vec::new(),
        unchanged: Vec::new(),
        affected_mods: Vec::new(),
        dependents: Vec::new(),
        unmet_requirements: Vec::new(),
        applied: false,
        result: None,
    };
    for wanted in requested {
        let found = installed
            .iter()
            .find(|m| m.unique_id.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| {
                AppError::new(
                    "CLI_MOD_NOT_FOUND",
                    AppErrorCategory::Validation,
                    format!("No mod with UniqueID '{wanted}' is in this profile"),
                )
            })?;
        if found.enabled == enable {
            plan.unchanged.push(mod_ref(found));
        } else {
            plan.changes.push(mod_ref(found));
            ids.push(
                ProfileComponentId::from_str(&found.profile_component_id)
                    .map_err(|e| AppError::internal("A mod has an invalid id", e.to_string()))?,
            );
        }
    }
    // The same review the app shows before it changes anything.
    if !ids.is_empty() {
        let impact = state.services.toggle.impact_many(&ids, enable)?;
        plan.affected_mods = impact.affected_mods;
        plan.dependents = impact.dependents;
        plan.unmet_requirements = impact.unmet_requirements;
    }
    let warned = !plan.dependents.is_empty() || !plan.unmet_requirements.is_empty();
    let verb = if enable { "Turn on" } else { "Turn off" };
    let already = if enable { "on" } else { "off" };

    if options.yes && !ids.is_empty() {
        let result = state.services.toggle.set_many_enabled(&ids, enable)?;
        plan.applied = true;
        let failed = !result.failed.is_empty();
        plan.result = Some(result);
        if options.json {
            emit(out, &plan);
        } else {
            let result = plan.result.as_ref().expect("set above");
            for name in &result.changed {
                let _ = writeln!(out, "{verb}: {name}: done");
            }
            for failure in &result.failed {
                let _ = writeln!(
                    out,
                    "{verb}: {}: not changed: {}",
                    failure.name, failure.message
                );
            }
            for item in &plan.unchanged {
                let _ = writeln!(out, "Already {already}: {}", item.unique_id);
            }
            print_warnings(out, &plan);
        }
        return Ok(if failed {
            FAILED
        } else if warned {
            ATTENTION
        } else {
            OK
        });
    }
    if options.json {
        emit(out, &plan);
    } else {
        for item in &plan.changes {
            let _ = writeln!(
                out,
                "{verb}: {} {} ({})",
                item.unique_id, item.version, item.name
            );
        }
        let named: Vec<&str> = plan.changes.iter().map(|m| m.name.as_str()).collect();
        let companions: Vec<&str> = plan
            .affected_mods
            .iter()
            .map(String::as_str)
            .filter(|name| !named.contains(name))
            .collect();
        if !companions.is_empty() {
            let _ = writeln!(
                out,
                "Also changes, because they came in the same package: {}",
                companions.join(", ")
            );
        }
        for item in &plan.unchanged {
            let _ = writeln!(out, "Already {already}: {}", item.unique_id);
        }
        print_warnings(out, &plan);
        if plan.changes.is_empty() {
            let _ = writeln!(out, "Nothing to change.");
        } else {
            let _ = writeln!(out, "Nothing changed yet. Run again with --yes to do this.");
        }
    }
    Ok(OK)
}

fn print_warnings(out: &mut dyn Write, plan: &CliTogglePlanDto) {
    if !plan.dependents.is_empty() {
        let _ = writeln!(
            out,
            "Warning: these enabled mods need it and will not load while it is off: {}",
            plan.dependents.join(", ")
        );
    }
    for line in &plan.unmet_requirements {
        let _ = writeln!(out, "Warning: {line}");
    }
}
