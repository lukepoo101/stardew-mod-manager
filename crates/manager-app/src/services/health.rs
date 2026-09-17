use crate::api::dto::{FindingDto, HealthSummaryDto};
use crate::error::AppResult;
use crate::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, LaunchSessionRepository, OperationRepository,
    PackageCatalogRepository, ProfileRepository, SmapiRepository,
};
use chrono::Utc;
use manager_core::dependency::evaluation::build_dependency_graph;
use manager_core::ids::ProfileId;
use manager_core::launch::SessionState;
use std::sync::Arc;

pub struct HealthService {
    game_repo: Arc<dyn GameInstallationRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    smapi_repo: Arc<dyn SmapiRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    session_repo: Arc<dyn LaunchSessionRepository>,
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
        }
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
                    let active_profile_id = self
                        .profile_repo
                        .get_game_profile_context(&gid)?
                        .and_then(|ctx| ctx.active_profile_id);
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

            // Check Missing Dependencies
            let mut manifests = Vec::new();
            let mut enabled_map = std::collections::HashMap::new();

            for pc in self.deployment_repo.list_profile_components(&profile.id)? {
                enabled_map.insert(pc.package_component_id, pc.enabled);
                if let Some(comp) = self
                    .package_repo
                    .get_package_component(&pc.package_component_id)?
                {
                    manifests.push(comp.manifest);
                }
            }

            let graph = build_dependency_graph(&manifests, None);
            for edge in &graph.edges {
                if (edge.edge_type == manager_core::dependency::DependencyEdgeType::Required
                    || edge.edge_type
                        == manager_core::dependency::DependencyEdgeType::ContentPackFor)
                    && graph.get_node(&edge.target_id).is_none()
                {
                    findings.push(FindingDto {
                        id: uuid::Uuid::new_v4().to_string(),
                        fingerprint: format!("missing_dep_{}_{}", edge.source_id, edge.target_id),
                        code: "MISSING_DEPENDENCY".to_string(),
                        severity: "error".to_string(),
                        category: "dependency".to_string(),
                        title: format!("Missing Required Dependency '{}'", edge.target_id),
                        summary: format!(
                            "Mod '{}' requires '{}', but it is not installed.",
                            edge.source_id, edge.target_id
                        ),
                        affected_entities: vec![edge.source_id.to_string()],
                        evidence: vec![format!("Required by {}", edge.source_id)],
                        observed_at: Utc::now().to_rfc3339(),
                    });
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

        let mut warning_count = 0;
        let mut error_count = 0;

        for f in &findings {
            match f.severity.as_str() {
                "critical" | "error" => error_count += 1,
                "warning" => warning_count += 1,
                _ => {}
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
            findings,
        })
    }
}
