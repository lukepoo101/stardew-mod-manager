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
            self.profile_repo
                .get_game_profile_context(gid)?
                .and_then(|ctx| ctx.active_profile_id)
        } else {
            None
        };

        let unresolved = self.operation_repo.list_unresolved_operations()?;
        let recovery_summary = unresolved
            .iter()
            .find(|op| op.state.requires_recovery())
            .map(|op| {
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
            app_version: self.app_version.clone(),
        })
    }

    pub fn set_onboarding_disposition(&self, disposition: OnboardingDisposition) -> AppResult<()> {
        let mut app_ctx = self.game_repo.get_app_context()?;
        app_ctx.onboarding_disposition = disposition;
        self.game_repo.save_app_context(&app_ctx)
    }
}
