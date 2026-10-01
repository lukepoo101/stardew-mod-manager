use crate::api::dto::{sort_findings, FindingDto, HealthSummaryDto};
use crate::error::AppResult;
use crate::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, LaunchSessionRepository, OperationRepository,
    PackageCatalogRepository, ProfileRepository, SmapiRepository,
};
use crate::services::runtime_observer::RuntimeObserver;
use chrono::Utc;
use manager_core::health::live::{
    check_requirements, find_duplicate_ids, LiveComponent, RequirementFinding, RequirementProblem,
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
        }
    }

    /// Enables warnings when the game or SMAPI changed since a profile last
    /// loaded its mods.
    pub fn with_runtime_observer(mut self, observer: Arc<RuntimeObserver>) -> Self {
        self.observer = Some(observer);
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
            }

            // Check whether the runtime changed since this profile last worked.
            if let Some(observer) = &self.observer {
                if let (Some(before), Ok(now)) = (
                    observer.recall(&profile.id)?,
                    observer.observe(&profile.game_installation_id),
                ) {
                    for change in runtime_changes(&before, &now) {
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
                            evidence: vec![format!("Last working: {}. Current: {}.", from, to)],
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
