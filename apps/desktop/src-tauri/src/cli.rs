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
//!   game running, another operation in progress)
//! - 3: it worked, but something needs attention: an interrupted operation
//!   waits for recovery, or health has warnings or errors

use crate::state::AppState;
use manager_app::api::dto::{BootstrapDto, ModListItemDto};
use manager_app::error::{AppError, AppErrorCategory, AppResult};
use manager_core::ids::{GameInstallationId, ProfileComponentId, ProfileId};
use manager_infra::paths::AppPaths;
use serde::Serialize;
use std::io::Write;
use std::str::FromStr;

pub const OK: i32 = 0;
pub const FAILED: i32 = 1;
pub const INVALID: i32 = 2;
pub const ATTENTION: i32 = 3;

const USAGE: &str = "\
Usage: stardew-mod-manager cli <command> [options]

Read-only:
  status                 Active game and profile, and any interrupted operation
  profiles               Profiles for the active game
  mods [--profile ID]    Mods in the active (or given) profile
  health                 Health findings for the active profile

Changes (shows the plan; add --yes to carry it out):
  enable <mod>...        Turn mods on, by UniqueID
  disable <mod>...       Turn mods off, by UniqueID

Options:
  --json                 Machine-readable output
  --yes                  Carry out a change instead of only showing it

Exit codes: 0 success, 1 failure, 2 invalid or refused, 3 needs attention.
The CLI never accepts credentials.
";

/// Runs one CLI invocation and returns the process exit code. `args` are the
/// words after `cli`.
pub fn run(args: &[String]) -> i32 {
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let state = match AppPaths::from_env_or_default().and_then(AppState::without_startup_recovery) {
        Ok(state) => state,
        Err(message) => {
            let _ = writeln!(err, "Could not open the manager's data: {message}");
            return FAILED;
        }
    };
    run_with(&state, args, &mut out, &mut err)
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
                        .ok_or("--profile needs a profile id")?
                        .to_string(),
                )
            }
            flag if flag.starts_with("--") => return Err(format!("Unknown option {flag}")),
            word => options.words.push(word.to_string()),
        }
    }
    Ok(options)
}

/// The testable core: runs against `state`, writing results to `out` and
/// problems to `err`.
pub fn run_with(
    state: &AppState,
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let options = match parse(args) {
        Ok(options) => options,
        Err(message) => {
            let _ = writeln!(err, "{message}\n\n{USAGE}");
            return INVALID;
        }
    };
    let Some((command, rest)) = options.words.split_first() else {
        let _ = write!(err, "{USAGE}");
        return INVALID;
    };
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
        other => {
            let _ = writeln!(err, "Unknown command '{other}'.\n\n{USAGE}");
            return INVALID;
        }
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            if options.json {
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

fn active_profile(state: &AppState, requested: Option<&str>) -> AppResult<ProfileId> {
    let id = match requested {
        Some(id) => id.to_string(),
        None => bootstrap(state)?
            .active_profile_id
            .ok_or_else(crate::ipc::no_active_profile)?,
    };
    ProfileId::from_str(&id).map_err(crate::ipc::invalid_profile_id)
}

fn status(state: &AppState, options: &Options, out: &mut dyn Write) -> AppResult<i32> {
    let boot = bootstrap(state)?;
    if options.json {
        emit(out, &boot);
    } else {
        let _ = writeln!(out, "Manager version: {}", boot.app_version);
        let _ = writeln!(
            out,
            "Game: {}",
            boot.active_game_installation_id
                .as_deref()
                .unwrap_or("none chosen")
        );
        let _ = writeln!(
            out,
            "Active profile: {}",
            boot.active_profile_id.as_deref().unwrap_or("none")
        );
        match &boot.recovery {
            Some(recovery) => {
                let _ = writeln!(
                    out,
                    "Interrupted operation: {} ({}), started {}. Open the app to recover it.",
                    recovery.kind, recovery.state, recovery.started_at
                );
            }
            None => {
                let _ = writeln!(out, "No interrupted operations.");
            }
        }
    }
    Ok(if boot.recovery.is_some() {
        ATTENTION
    } else {
        OK
    })
}

fn profiles(state: &AppState, options: &Options, out: &mut dyn Write) -> AppResult<i32> {
    let boot = bootstrap(state)?;
    let game = boot
        .active_game_installation_id
        .ok_or_else(crate::ipc::no_active_game)?;
    let game =
        GameInstallationId::from_str(&game).map_err(crate::ipc::invalid_game_installation_id)?;
    let list = state.services.profiles.list_profiles(&game)?;
    if options.json {
        emit(out, &list);
    } else {
        for profile in &list {
            let active = boot.active_profile_id.as_deref() == Some(profile.id.as_str());
            let _ = writeln!(
                out,
                "{}{}\t{}\t{} mod(s)",
                if active { "* " } else { "  " },
                profile.id,
                profile.name,
                profile.mod_count
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

#[derive(Serialize)]
struct TogglePlan {
    enable: bool,
    /// Mods that will change, as UniqueID and version.
    changes: Vec<(String, String)>,
    /// Mods already in the requested state.
    unchanged: Vec<String>,
    applied: bool,
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
    if bootstrap(state)?.recovery.is_some() {
        return Err(AppError::new(
            "RECOVERY_PENDING",
            AppErrorCategory::Recovery,
            "An interrupted operation needs recovery first; open the app to recover it",
        ));
    }
    let profile = active_profile(state, options.profile.as_deref())?;
    let installed = list_mods(state, &profile)?;
    let mut ids = Vec::new();
    let mut plan = TogglePlan {
        enable,
        changes: Vec::new(),
        unchanged: Vec::new(),
        applied: false,
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
            plan.unchanged.push(found.unique_id.clone());
        } else {
            plan.changes
                .push((found.unique_id.clone(), found.version.clone()));
            ids.push(
                ProfileComponentId::from_str(&found.profile_component_id)
                    .map_err(|e| AppError::internal("A mod has an invalid id", e.to_string()))?,
            );
        }
    }
    let verb = if enable { "Turn on" } else { "Turn off" };
    if options.yes && !ids.is_empty() {
        let result = state.services.toggle.set_many_enabled(&ids, enable)?;
        plan.applied = true;
        if options.json {
            emit(out, &serde_json::json!({ "plan": plan, "result": result }));
        } else {
            for (id, version) in &plan.changes {
                let _ = writeln!(out, "{verb}: {id} {version} — done");
            }
        }
        return Ok(OK);
    }
    if options.json {
        emit(out, &plan);
    } else {
        for (id, version) in &plan.changes {
            let _ = writeln!(out, "{verb}: {id} {version}");
        }
        for id in &plan.unchanged {
            let _ = writeln!(out, "Already {}: {id}", if enable { "on" } else { "off" });
        }
        if plan.changes.is_empty() {
            let _ = writeln!(out, "Nothing to change.");
        } else {
            let _ = writeln!(out, "Nothing changed yet. Run again with --yes to do this.");
        }
    }
    Ok(OK)
}
