use crate::api::dto::BootstrapDto;
use crate::error::AppResult;
use crate::ports::repositories::{
    GameInstallationRepository, OperationRepository, ProfileRepository,
};
use manager_core::profile::OnboardingDisposition;
use std::sync::Arc;

pub struct BootstrapService {
    game_repo: Arc<dyn GameInstallationRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    app_version: String,
}

impl BootstrapService {
    pub fn new(
        game_repo: Arc<dyn GameInstallationRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
        operation_repo: Arc<dyn OperationRepository>,
        app_version: impl Into<String>,
    ) -> Self {
        Self {
            game_repo,
            profile_repo,
            operation_repo,
            app_version: app_version.into(),
        }
    }

    pub fn get_bootstrap(&self) -> AppResult<BootstrapDto> {
        let app_ctx = self.game_repo.get_app_context()?;
        let active_game_id = app_ctx.active_game_installation_id;

        let active_profile_id = if let Some(ref gid) = active_game_id {
            super::profiles::resolve_active_profile(self.profile_repo.as_ref(), gid)?
        } else {
            None
        };

        let unresolved = self.operation_repo.list_unresolved_operations()?;
        let interrupted = unresolved.iter().find(|op| op.state.requires_recovery());
        let recovery = match interrupted {
            Some(op) => Some(crate::api::dto::RecoveryDetailDto {
                operation_id: op.id.to_string(),
                kind: format!("{:?}", op.kind),
                state: format!("{:?}", op.state),
                error_code: op.error_code.clone(),
                error_message: op.error_json.clone(),
                started_at: op.created_at.to_rfc3339(),
                steps: self
                    .operation_repo
                    .list_operation_steps(&op.id)?
                    .into_iter()
                    .map(|step| crate::api::dto::OperationStepDto {
                        step_index: step.step_index,
                        step_kind: step.step_kind,
                        state: format!("{:?}", step.state),
                        started_at: step.started_at.map(|at| at.to_rfc3339()),
                        completed_at: step.completed_at.map(|at| at.to_rfc3339()),
                    })
                    .collect(),
            }),
            None => None,
        };
        let recovery_summary = interrupted.map(|op| {
            format!(
                "Operation {} ({:?}) requires recovery: {}",
                op.id,
                op.kind,
                op.error_code.as_deref().unwrap_or("unknown failure")
            )
        });

        let disp_str = match app_ctx.onboarding_disposition {
            OnboardingDisposition::NotStarted => "not_started",
            OnboardingDisposition::Skipped => "skipped",
            OnboardingDisposition::Completed => "completed",
        };

        Ok(BootstrapDto {
            onboarding_disposition: disp_str.to_string(),
            active_game_installation_id: active_game_id.map(|id| id.to_string()),
            active_profile_id: active_profile_id.map(|id| id.to_string()),
            recovery_summary,
            recovery,
            app_version: self.app_version.clone(),
        })
    }

    pub fn set_onboarding_disposition(&self, disposition: OnboardingDisposition) -> AppResult<()> {
        let mut app_ctx = self.game_repo.get_app_context()?;
        app_ctx.onboarding_disposition = disposition;
        self.game_repo.save_app_context(&app_ctx)
    }
}
