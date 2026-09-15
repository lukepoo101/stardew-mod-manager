use crate::api::dto::{LaunchSessionSummaryDto, ProfileOverviewDto};
use crate::error::{AppError, AppResult};
use crate::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, LaunchSessionRepository, ProfileRepository,
};
use crate::services::health::HealthService;
use crate::services::profiles::ProfilesService;
use crate::services::smapi::SmapiService;
use manager_core::ids::ProfileId;
use std::sync::Arc;

pub struct ProfileQueries {
    profile_service: Arc<ProfilesService>,
    health_service: Arc<HealthService>,
    smapi_service: Arc<SmapiService>,
    game_repo: Arc<dyn GameInstallationRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    session_repo: Arc<dyn LaunchSessionRepository>,
}

impl ProfileQueries {
    pub fn new(
        profile_service: Arc<ProfilesService>,
        health_service: Arc<HealthService>,
        smapi_service: Arc<SmapiService>,
        game_repo: Arc<dyn GameInstallationRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        session_repo: Arc<dyn LaunchSessionRepository>,
    ) -> Self {
        Self {
            profile_service,
            health_service,
            smapi_service,
            game_repo,
            profile_repo,
            deployment_repo,
            session_repo,
        }
    }

    pub fn get_profile_overview(&self, profile_id: &ProfileId) -> AppResult<ProfileOverviewDto> {
        let profile_summary = self
            .profile_service
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;

        let profile = self.profile_repo.get_profile(profile_id)?.unwrap();
        let game = self
            .game_repo
            .get_game(&profile.game_installation_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "Game installation not found"))?;

        let game_summary = crate::api::dto::GameInstallationSummaryDto {
            id: game.id.to_string(),
            canonical_root: game.canonical_root.to_string_lossy().to_string(),
            operating_system: format!("{:?}", game.operating_system).to_lowercase(),
            storefront: format!("{:?}", game.storefront).to_lowercase(),
            management_mode: format!("{:?}", game.management_mode).to_lowercase(),
            created_at: game.created_at.to_rfc3339(),
        };

        let mod_count = self
            .deployment_repo
            .list_profile_components(profile_id)?
            .len();
        let smapi_status = self.smapi_service.get_smapi_status(&game.id)?;
        let health_summary = self.health_service.get_health_summary(Some(profile_id))?;

        let last_session = self
            .session_repo
            .get_latest_launch_session(Some(profile_id))?
            .map(|s| LaunchSessionSummaryDto {
                id: s.id.to_string(),
                state: format!("{:?}", s.state).to_lowercase(),
                launched_at: s.launched_at.to_rfc3339(),
                verification_result: s.verification_result.map(|v| v.details),
            });

        Ok(ProfileOverviewDto {
            profile: profile_summary,
            game: game_summary,
            mod_count,
            smapi_status,
            health_summary,
            last_session,
        })
    }
}
