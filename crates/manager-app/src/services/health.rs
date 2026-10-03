use crate::api::dto::{sort_findings, FindingDto, HealthSummaryDto};
use crate::error::AppResult;
use crate::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, LaunchSessionRepository, OperationRepository,
    PackageCatalogRepository, ProfileRepository, SmapiRepository,
};
use crate::services::runtime_observer::RuntimeObserver;
use chrono::Utc;
use manager_core::health::live::{
    check_optional, check_requirements, find_duplicate_ids, LiveComponent, RequirementFinding,
    RequirementProblem,
};
use manager_core::ids::ProfileId;
use manager_core::launch::SessionState;
use manager_core::launch::{runtime_changes, RuntimeChange};
use std::sync::Arc;

pub struct HealthService {
    game_repo: Arc<dyn GameInstallationRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    smapi_repo: Arc<dyn SmapiRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    session_repo: Arc<dyn LaunchSessionRepository>,
    observer: Option<Arc<RuntimeObserver>>,
    references: Option<crate::services::ReferenceRecipes>,
}

impl HealthService {
    pub fn new(
        game_repo: Arc<dyn GameInstallationRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        smapi_repo: Arc<dyn SmapiRepository>,
        operation_repo: Arc<dyn OperationRepository>,
        session_repo: Arc<dyn LaunchSessionRepository>,
    ) -> Self {
        Self {
            game_repo,
            profile_repo,
            deployment_repo,
            package_repo,
            smapi_repo,
            operation_repo,
            session_repo,
            observer: None,
            references: None,
        }
    }

    /// Enables warnings when the game or SMAPI changed since a profile last
    /// loaded its mods.
    pub fn with_runtime_observer(mut self, observer: Arc<RuntimeObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Warns while required mods from a profile's group reference (such as an
    /// imported bundle's list) are not installed.
    pub fn with_references(
        mut self,
        preferences: Arc<dyn crate::ports::repositories::PreferencesRepository>,
    ) -> Self {
        self.references = Some(crate::services::ReferenceRecipes::new(preferences));
        self
    }

    pub fn get_health_summary(
        &self,
        profile_id: Option<&ProfileId>,
    ) -> AppResult<HealthSummaryDto> {
        let mut findings = Vec::new();

        // 1. Check RecoveryRequired
        let unresolved = self.operation_repo.list_unresolved_operations()?;
        for op in &unresolved {
            if op.state.requires_recovery() {
                findings.push(FindingDto {
                    id: uuid::Uuid::new_v4().to_string(),
                    fingerprint: format!("recovery_required_{}", op.id),
                    code: "RECOVERY_REQUIRED".to_string(),
                    severity: "critical".to_string(),
                    category: "recovery".to_string(),
                    title: "Interrupted Operation Requires Recovery".to_string(),
                    summary: format!(
                        "Operation {} was interrupted and needs recovery before further changes.",
                        op.id
                    ),
                    affected_entities: vec![op.id.to_string()],
                    evidence: vec![op.error_json.clone().unwrap_or_default()],
                    observed_at: Utc::now().to_rfc3339(),
                });
            }
        }

        // 2. Check active profile and game
        // A repository failure while resolving the active profile is a real
        // failure: reporting "no active profile" instead would silently change
        // what the health report claims about the installation.
        let active_profile = if let Some(pid) = profile_id {
            self.profile_repo.get_profile(pid)?
        } else {
            let app_ctx = self.game_repo.get_app_context()?;
            match app_ctx.active_game_installation_id {
                Some(gid) => {
                    let active_profile_id =
                        super::profiles::resolve_active_profile(self.profile_repo.as_ref(), &gid)?;
                    match active_profile_id {
                        Some(pid) => self.profile_repo.get_profile(&pid)?,
                        None => None,
                    }
                }
                None => None,
            }
        };

        if let Some(profile) = active_profile {
            // Check SMAPI
            if self
                .smapi_repo
                .get_smapi_installation(&profile.game_installation_id)?
                .is_none()
            {
                findings.push(FindingDto {
                    id: uuid::Uuid::new_v4().to_string(),
                    fingerprint: "smapi_missing".to_string(),
                    code: "SMAPI_MISSING".to_string(),
                    severity: "warning".to_string(),
                    category: "runtime".to_string(),
                    title: "SMAPI Not Installed".to_string(),
                    summary: "SMAPI is required to launch modded games in this profile."
                        .to_string(),
                    affected_entities: vec![profile.id.to_string()],
                    evidence: vec!["No SMAPI installation record in state database".to_string()],
                    observed_at: Utc::now().to_rfc3339(),
                });
            }

            // Required dependencies, content-pack hosts and duplicate
            // UniqueIDs among the profile's live (enabled) components.
            let mut installed = Vec::new();
            for pc in self.deployment_repo.list_profile_components(&profile.id)? {
                if let Some(comp) = self
                    .package_repo
                    .get_package_component(&pc.package_component_id)?
                {
                    installed.push((comp, pc.enabled));
                }
            }
            let live: Vec<LiveComponent<'_>> = installed
                .iter()
                .map(|(comp, enabled)| LiveComponent {
                    manifest: &comp.manifest,
                    enabled: *enabled,
                    folder: &comp.relative_component_root,
                })
                .collect();
            for finding in check_requirements(&live) {
                findings.push(requirement_finding(&finding));
            }
            for finding in check_optional(&live) {
                findings.push(optional_finding(&finding));
            }
            for duplicate in find_duplicate_ids(&live) {
                findings.push(FindingDto {
                    id: uuid::Uuid::new_v4().to_string(),
                    fingerprint: format!("duplicate_id_{}", duplicate.unique_id.to_lowercase()),
                    code: "DUPLICATE_UNIQUE_ID".to_string(),
                    severity: "error".to_string(),
                    category: "dependency".to_string(),
                    title: format!(
                        "{} enabled mods share the ID '{}'",
                        duplicate.copies.len(),
                        duplicate.unique_id
                    ),
                    summary: "SMAPI loads only one mod per ID and skips the rest, so which copy runs is not up to you. Disable or remove all but one copy.".to_string(),
                    affected_entities: duplicate.copies.iter().map(|c| c.name.clone()).collect(),
                    evidence: duplicate
                        .copies
                        .iter()
                        .map(|c| format!("{} {} in folder '{}'", c.name, c.version, c.folder))
                        .collect(),
                    observed_at: Utc::now().to_rfc3339(),
                });
            }

            // Minimum SMAPI and game versions declared by enabled mods.
            {
                let mut needs_smapi = Vec::new();
                let mut needs_game = Vec::new();
                for (comp, _) in installed.iter().filter(|(_, enabled)| *enabled) {
                    if let Some(min) = &comp.manifest.minimum_api_version {
                        needs_smapi.push((comp.name.clone(), min.clone()));
                    }
                    if let Some(min) = &comp.manifest.minimum_game_version {
                        needs_game.push((comp.name.clone(), min.clone()));
                    }
                }
                let smapi_version = self
                    .smapi_repo
                    .get_smapi_installation(&profile.game_installation_id)?
                    .map(|i| i.release_version);
                let game_version = self.observer.as_ref().and_then(|o| {
                    o.observe(&profile.game_installation_id)
                        .ok()
                        .and_then(|v| v.game_version)
                });
                for (code, what, required, installed, workflow) in [
                    (
                        "MOD_NEEDS_NEWER_SMAPI",
                        "SMAPI",
                        &needs_smapi,
                        smapi_version.as_deref(),
                        "Update SMAPI",
                    ),
                    (
                        "MOD_NEEDS_NEWER_GAME",
                        "Stardew Valley",
                        &needs_game,
                        game_version.as_deref(),
                        "Update the game",
                    ),
                ] {
                    if required.is_empty() {
                        continue;
                    }
                    let check = manager_core::health::minimums::check_minimums(required, installed);
                    if let (false, Some(strongest), Some(have)) =
                        (check.too_old_for.is_empty(), &check.strongest, installed)
                    {
                        findings.push(FindingDto {
                            id: uuid::Uuid::new_v4().to_string(),
                            fingerprint: format!("{}_{}_{}", code.to_lowercase(), have, strongest),
                            code: code.to_string(),
                            severity: "error".to_string(),
                            category: "compatibility".to_string(),
                            title: format!(
                                "{} mod(s) need {what} {strongest} or newer",
                                check.too_old_for.len()
                            ),
                            summary: format!(
                                "{have} is installed. {what} will not load these mods until it is updated. {workflow} to at least {strongest}."
                            ),
                            affected_entities: check.too_old_for.iter().map(|(n, _)| n.clone()).collect(),
                            evidence: check
                                .too_old_for
                                .iter()
                                .map(|(name, min)| format!("{name} declares a minimum of {min}"))
                                .collect(),
                            observed_at: Utc::now().to_rfc3339(),
                        });
                    }
                    if !check.unassessed.is_empty() {
                        findings.push(FindingDto {
                            id: uuid::Uuid::new_v4().to_string(),
                            fingerprint: format!("{}_unassessed", code.to_lowercase()),
                            code: format!("{code}_UNASSESSED"),
                            severity: "info".to_string(),
                            category: "compatibility".to_string(),
                            title: format!("Could not check some mods' minimum {what} version"),
                            summary: if installed.is_none() {
                                format!("The installed {what} version is not known, so these mods' minimums could not be compared.")
                            } else {
                                "These mods declare a minimum version that could not be read.".to_string()
                            },
                            affected_entities: check.unassessed.clone(),
                            evidence: check.unassessed.iter().map(|n| format!("{n}: not assessed")).collect(),
                            observed_at: Utc::now().to_rfc3339(),
                        });
                    }
                }

                // Whether this SMAPI and game are a pair this manager tested.
                if let Some(smapi) = smapi_version.as_deref() {
                    use manager_core::health::runtime_pair::{assess_runtime_pair, RuntimePair};
                    let policy = manager_core::smapi::get_pinned_smapi_release();
                    let pair = assess_runtime_pair(
                        smapi,
                        game_version.as_deref(),
                        &policy.version,
                        &policy.supported_game_version,
                    );
                    let game_text = game_version.as_deref().unwrap_or("an unknown version");
                    let tested = format!(
                        "This manager is tested with SMAPI {} on Stardew Valley {}.",
                        policy.version, policy.supported_game_version
                    );
                    let finding = match pair {
                        RuntimePair::Tested => None,
                        RuntimePair::GameTooOld { minimum } => Some((
                            "SMAPI_GAME_TOO_OLD",
                            "warning",
                            format!("Stardew Valley {game_text} is older than SMAPI {smapi} supports"),
                            format!("SMAPI {smapi} needs Stardew Valley {minimum} or newer. Update the game before playing modded."),
                        )),
                        RuntimePair::Untested => Some((
                            "RUNTIME_PAIR_UNTESTED",
                            "info",
                            format!("SMAPI {smapi} with Stardew Valley {game_text} is not a tested pair"),
                            format!("{tested} This pair has not been tested by it, which does not mean it is broken."),
                        )),
                        RuntimePair::Unknown => Some((
                            "RUNTIME_PAIR_UNASSESSED",
                            "info",
                            "Could not check SMAPI against the game version".to_string(),
                            format!("SMAPI {smapi} and Stardew Valley {game_text} could not be compared with what this manager tested. {tested}"),
                        )),
                    };
                    if let Some((code, severity, title, summary)) = finding {
                        findings.push(FindingDto {
                            id: uuid::Uuid::new_v4().to_string(),
                            fingerprint: format!(
                                "{}_{}_{}",
                                code.to_lowercase(),
                                smapi,
                                game_version.as_deref().unwrap_or("unknown")
                            ),
                            code: code.to_string(),
                            severity: severity.to_string(),
                            category: "compatibility".to_string(),
                            title,
                            summary,
                            affected_entities: vec![profile.game_installation_id.to_string()],
                            evidence: vec![
                                format!("Installed SMAPI: {smapi} (manager record)"),
                                format!("Game version: {game_text} (read from the game files)"),
                                format!(
                                    "Tested pair: SMAPI {} on Stardew Valley {} (this manager's release policy)",
                                    policy.version, policy.supported_game_version
                                ),
                            ],
                            observed_at: Utc::now().to_rfc3339(),
                        });
                    }
                }
            }

            // Required mods from the group reference that are not installed.
            if let Some(references) = &self.references {
                if let Some(reference) = references.get(&profile.id)? {
                    if let Ok(recipe) =
                        manager_core::recipe::ProfileRecipe::parse(&reference.recipe_json)
                    {
                        let present: std::collections::HashSet<String> = installed
                            .iter()
                            .map(|(comp, _)| comp.unique_id.as_str().to_lowercase())
                            .collect();
                        let unresolved: Vec<_> = recipe
                            .components
                            .iter()
                            .filter(|c| !c.optional)
                            .filter(|c| !present.contains(&c.unique_id.to_lowercase()))
                            .filter(|c| {
                                !reference
                                    .accepted
                                    .contains(&format!("missing:{}:{}:", c.unique_id, c.version))
                            })
                            .collect();
                        if !unresolved.is_empty() {
                            findings.push(FindingDto {
                                id: uuid::Uuid::new_v4().to_string(),
                                fingerprint: format!(
                                    "reference_missing_{}",
                                    unresolved
                                        .iter()
                                        .map(|c| c.unique_id.to_lowercase())
                                        .collect::<Vec<_>>()
                                        .join(",")
                                ),
                                code: "REFERENCE_MODS_MISSING".to_string(),
                                severity: "warning".to_string(),
                                category: "dependency".to_string(),
                                title: format!(
                                    "{} mod(s) from the group's list are not installed",
                                    unresolved.len()
                                ),
                                summary: format!(
                                    "This profile's group reference \"{}\" lists them as required. Install them, or accept the difference on the Profiles page if the group does not need them.",
                                    recipe.profile_name
                                ),
                                affected_entities: unresolved
                                    .iter()
                                    .map(|c| c.unique_id.clone())
                                    .collect(),
                                evidence: unresolved
                                    .iter()
                                    .map(|c| format!("{} {} ({})", c.name, c.version, c.unique_id))
                                    .collect(),
                                observed_at: Utc::now().to_rfc3339(),
                            });
                        }
                    }
                }
            }

            // A game version the user set is always visible as such, and
            // flagged when detection has changed since it was set.
            if let Some(observer) = &self.observer {
                if let Ok(Some(status)) =
                    observer.game_version_override(&profile.game_installation_id)
                {
                    let detected = status
                        .observed_now
                        .clone()
                        .unwrap_or_else(|| "unreadable".to_string());
                    let stale = status.is_stale();
                    findings.push(FindingDto {
                        id: uuid::Uuid::new_v4().to_string(),
                        fingerprint: format!(
                            "game_version_override_{}_{}",
                            status.game_version.value, detected
                        ),
                        code: if stale {
                            "GAME_VERSION_OVERRIDE_STALE"
                        } else {
                            "GAME_VERSION_OVERRIDDEN"
                        }
                        .to_string(),
                        severity: if stale { "warning" } else { "info" }.to_string(),
                        category: "runtime".to_string(),
                        title: if stale {
                            "The game version you set may be out of date".to_string()
                        } else {
                            "The game version is set by you".to_string()
                        },
                        summary: if stale {
                            format!(
                                "You set Stardew Valley {} (detected then: {}). Detection now reads {}, so check whether your setting still applies.",
                                status.game_version.value,
                                status.game_version.observed_then.as_deref().unwrap_or("unreadable"),
                                detected
                            )
                        } else {
                            format!(
                                "Checks use Stardew Valley {} as you set it, instead of the detected {}.",
                                status.game_version.value, detected
                            )
                        },
                        affected_entities: vec![profile.game_installation_id.to_string()],
                        evidence: vec![
                            format!("Set {}", status.game_version.set_at),
                            format!("Reason: {}", if status.game_version.reason.is_empty() { "none given" } else { &status.game_version.reason }),
                        ],
                        observed_at: chrono::Utc::now().to_rfc3339(),
                    });
                }
            }

            // Check whether the runtime changed since this profile last worked.
            if let Some(observer) = &self.observer {
                if let (Some(before), Ok(now)) = (
                    observer.recall(&profile.id)?,
                    observer.observe(&profile.game_installation_id),
                ) {
                    let recorded_at = observer.recalled_at(&profile.id)?;
                    let tested_smapi = manager_core::smapi::get_pinned_smapi_release().version;
                    for change in runtime_changes(&before, &now) {
                        let is_smapi = matches!(change, RuntimeChange::Smapi { .. });
                        let (code, what, from, to) = match change {
                            RuntimeChange::Game { before, now } => {
                                ("RUNTIME_GAME_CHANGED", "Stardew Valley", before, now)
                            }
                            RuntimeChange::Smapi { before, now } => {
                                ("RUNTIME_SMAPI_CHANGED", "SMAPI", before, now)
                            }
                        };
                        findings.push(FindingDto {
                            id: uuid::Uuid::new_v4().to_string(),
                            fingerprint: format!("{}_{}_{}", code.to_lowercase(), from, to),
                            code: code.to_string(),
                            severity: "warning".to_string(),
                            category: "runtime".to_string(),
                            title: format!("{} changed since this profile last worked", what),
                            summary: format!(
                                "{} was {} when this profile last loaded its mods and is {} now. Mods may need updates.",
                                what, from, to
                            ),
                            affected_entities: vec![profile.id.to_string()],
                            evidence: {
                                let mut lines = vec![format!(
                                    "Last working: {}{}. Current: {}.",
                                    from,
                                    recorded_at
                                        .as_deref()
                                        .map(|at| format!(" (recorded {at})"))
                                        .unwrap_or_default(),
                                    to
                                )];
                                if is_smapi {
                                    lines.push(if to == tested_smapi {
                                        format!("SMAPI {to} is the version this manager is tested with.")
                                    } else {
                                        format!("SMAPI {to} is not the version this manager is tested with ({tested_smapi}).")
                                    });
                                }
                                lines
                            },
                            observed_at: Utc::now().to_rfc3339(),
                        });
                    }
                }
            }

            // Check Verification Unavailable
            if let Some(session) = self
                .session_repo
                .get_latest_launch_session(Some(&profile.id))?
            {
                if session.state == SessionState::VerificationUnavailable {
                    findings.push(FindingDto {
                        id: uuid::Uuid::new_v4().to_string(),
                        fingerprint: format!("verification_unavailable_{}", session.id),
                        code: "VERIFICATION_UNAVAILABLE".to_string(),
                        severity: "warning".to_string(),
                        category: "verification".to_string(),
                        title: "Session Verification Unavailable".to_string(),
                        summary: "The SMAPI log file was not accessible or did not produce load confirmation.".to_string(),
                        affected_entities: vec![session.id.to_string()],
                        evidence: vec!["Verification timeout or missing log".to_string()],
                        observed_at: Utc::now().to_rfc3339(),
                    });
                }
            }
        }

        sort_findings(&mut findings);

        let mut warning_count = 0;
        let mut error_count = 0;
        let mut info_count = 0;

        for f in &findings {
            match f.severity.as_str() {
                "critical" | "error" => error_count += 1,
                "warning" => warning_count += 1,
                _ => info_count += 1,
            }
        }

        let status = if error_count > 0 {
            "error".to_string()
        } else if warning_count > 0 {
            "warning".to_string()
        } else {
            "healthy".to_string()
        };

        Ok(HealthSummaryDto {
            status,
            warning_count,
            error_count,
            info_count,
            findings,
        })
    }
}

/// An informational finding for an optional dependency that is not met.
/// The mod still loads; only what it does with the other mod is affected.
/// Manifests do not say which features those are, so neither does this.
fn optional_finding(finding: &RequirementFinding) -> FindingDto {
    let minimum = finding
        .minimum
        .as_deref()
        .map(|m| format!(" {m} or newer"))
        .unwrap_or_default();
    let state = match &finding.problem {
        RequirementProblem::Missing => "is not installed".to_string(),
        RequirementProblem::Disabled => "is installed but turned off".to_string(),
        RequirementProblem::TooOld { installed } => {
            format!("is at {installed}, older than it asks for")
        }
        RequirementProblem::Unassessed { installed } => {
            format!("is at {installed}, which could not be compared with what it asks for")
        }
    };
    FindingDto {
        id: uuid::Uuid::new_v4().to_string(),
        fingerprint: format!(
            "optional_dep_{}_{}",
            finding.dependent_id, finding.required_id
        ),
        code: "OPTIONAL_DEPENDENCY_UNMET".to_string(),
        severity: "info".to_string(),
        category: "dependency".to_string(),
        title: format!(
            "Optional '{}' for '{}' {}",
            finding.required_id,
            finding.dependent,
            match finding.problem {
                RequirementProblem::Missing => "is not installed",
                RequirementProblem::Disabled => "is disabled",
                RequirementProblem::TooOld { .. } => "is older than suggested",
                RequirementProblem::Unassessed { .. } => "could not be checked",
            }
        ),
        summary: format!(
            "Mod '{}' can work with '{}'{minimum}, which {state}. '{}' still loads; anything it does together with '{}' will not be available. Its manifest does not say what that is.",
            finding.dependent, finding.required_id, finding.dependent, finding.required_id
        ),
        affected_entities: vec![finding.dependent_id.clone()],
        evidence: vec![format!(
            "Optional dependency of {} (IsRequired: false)",
            finding.dependent_id
        )],
        observed_at: Utc::now().to_rfc3339(),
    }
}

/// A health finding for one unmet requirement of an enabled mod.
fn requirement_finding(finding: &RequirementFinding) -> FindingDto {
    let what = if finding.host {
        "framework"
    } else {
        "dependency"
    };
    let minimum = finding
        .minimum
        .as_deref()
        .map(|m| format!(" {m} or newer"))
        .unwrap_or_default();
    let (code, severity, title, summary) = match &finding.problem {
        RequirementProblem::Missing => (
            "MISSING_DEPENDENCY",
            "error",
            format!("Missing Required Dependency '{}'", finding.required_id),
            format!(
                "Mod '{}' requires '{}'{minimum}, but it is not installed.",
                finding.dependent, finding.required_id
            ),
        ),
        RequirementProblem::Disabled => (
            "DEPENDENCY_DISABLED",
            "error",
            format!("Required {what} '{}' is disabled", finding.required_id),
            format!(
                "Mod '{}' needs '{}', which is installed but turned off. Enable it, or disable '{}' too.",
                finding.dependent, finding.required_id, finding.dependent
            ),
        ),
        RequirementProblem::TooOld { installed } => (
            "DEPENDENCY_TOO_OLD",
            "error",
            format!("Required {what} '{}' is too old", finding.required_id),
            format!(
                "Mod '{}' needs '{}'{minimum}, but {installed} is installed. Update '{}'.",
                finding.dependent, finding.required_id, finding.required_id
            ),
        ),
        RequirementProblem::Unassessed { installed } => (
            "DEPENDENCY_UNASSESSED",
            "info",
            format!("Could not check the version '{}' needs", finding.dependent),
            format!(
                "Mod '{}' asks for '{}'{minimum} and {installed} is installed, but the versions could not be compared. This has not been checked.",
                finding.dependent, finding.required_id
            ),
        ),
    };
    let relation = if finding.host {
        format!(
            "{} is a content pack for {}",
            finding.dependent, finding.required_id
        )
    } else {
        format!("Required by {}", finding.dependent_id)
    };
    FindingDto {
        id: uuid::Uuid::new_v4().to_string(),
        fingerprint: match finding.problem {
            RequirementProblem::Missing => {
                format!(
                    "missing_dep_{}_{}",
                    finding.dependent_id, finding.required_id
                )
            }
            _ => format!(
                "{}_{}_{}",
                code.to_lowercase(),
                finding.dependent_id,
                finding.required_id
            ),
        },
        code: code.to_string(),
        severity: severity.to_string(),
        category: "dependency".to_string(),
        title,
        summary,
        affected_entities: vec![finding.dependent_id.clone()],
        evidence: vec![relation],
        observed_at: Utc::now().to_rfc3339(),
    }
}
