use crate::api::dto::ProfileSummaryDto;
use crate::error::{AppError, AppResult};
use crate::ports::repositories::{
    AtomicMutationStore, DeploymentRepository, ProfileCreateCommit, ProfileRepository,
};
use chrono::Utc;
use manager_core::ids::{GameInstallationId, OperationId, ProfileId};
use manager_core::operation::OperationEffect;
use manager_core::profile::{GameProfileContext, Profile, ProfileState};
use std::str::FromStr;
use std::sync::Arc;

pub struct ProfilesService {
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    mutation_store: Arc<dyn AtomicMutationStore>,
}

impl ProfilesService {
    pub fn new(
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        mutation_store: Arc<dyn AtomicMutationStore>,
    ) -> Self {
        Self {
            profile_repo,
            deployment_repo,
            mutation_store,
        }
    }

    pub fn list_profiles(&self, game_id: &GameInstallationId) -> AppResult<Vec<ProfileSummaryDto>> {
        let profiles = self.profile_repo.list_profiles(game_id)?;
        let mut dtos = Vec::with_capacity(profiles.len());

        for p in profiles {
            let mod_count = self.deployment_repo.list_profile_components(&p.id)?.len();
            dtos.push(Self::profile_to_dto(&p, mod_count));
        }

        Ok(dtos)
    }

    pub fn get_profile(&self, id: &ProfileId) -> AppResult<Option<ProfileSummaryDto>> {
        if let Some(p) = self.profile_repo.get_profile(id)? {
            let mod_count = self.deployment_repo.list_profile_components(&p.id)?.len();
            Ok(Some(Self::profile_to_dto(&p, mod_count)))
        } else {
            Ok(None)
        }
    }

    pub fn get_active_profile(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Option<ProfileSummaryDto>> {
        if let Some(ctx) = self.profile_repo.get_game_profile_context(game_id)? {
            if let Some(active_id) = ctx.active_profile_id {
                return self.get_profile(&active_id);
            }
        }
        Ok(None)
    }

    pub fn create_profile(
        &self,
        game_id: &GameInstallationId,
        name: &str,
        description: Option<&str>,
    ) -> AppResult<ProfileSummaryDto> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(AppError::validation(
                "EMPTY_PROFILE_NAME",
                "Profile name cannot be empty",
            ));
        }

        let existing = self.profile_repo.list_profiles(game_id)?;
        if existing
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(trimmed_name))
        {
            return Err(AppError::validation(
                "DUPLICATE_PROFILE_NAME",
                format!("A profile named '{}' already exists", trimmed_name),
            ));
        }

        let profile_id = ProfileId::new();
        let profile = Profile {
            id: profile_id,
            game_installation_id: *game_id,
            name: trimmed_name.to_string(),
            description: description.map(|s| s.trim().to_string()),
            revision: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            state: ProfileState::Active,
        };

        let effect = OperationEffect {
            id: uuid::Uuid::new_v4().to_string(),
            operation_id: OperationId::new(),
            profile_id: Some(profile_id),
            entity_type: "profile".to_string(),
            entity_id: profile_id.to_string(),
            change_kind: "ProfileCreated".to_string(),
            before_json: None,
            after_json: serde_json::to_string(&profile).ok(),
            occurred_at: Utc::now(),
        };

        self.mutation_store
            .commit_profile_create(ProfileCreateCommit {
                profile: profile.clone(),
                set_as_active: false,
                set_as_default: false,
                effects: vec![effect],
            })?;

        Ok(Self::profile_to_dto(&profile, 0))
    }

    pub fn switch_active_profile(
        &self,
        game_id: &GameInstallationId,
        profile_id: &ProfileId,
    ) -> AppResult<()> {
        let profile = self.profile_repo.get_profile(profile_id)?.ok_or_else(|| {
            AppError::validation(
                "PROFILE_NOT_FOUND",
                format!("Profile {} not found", profile_id),
            )
        })?;

        if &profile.game_installation_id != game_id {
            return Err(AppError::validation(
                "PROFILE_GAME_MISMATCH",
                "Profile does not belong to the active game",
            ));
        }

        let mut ctx = self
            .profile_repo
            .get_game_profile_context(game_id)?
            .unwrap_or(GameProfileContext {
                game_installation_id: *game_id,
                active_profile_id: None,
                default_profile_id: None,
                last_active_profile_id: None,
            });

        ctx.last_active_profile_id = ctx.active_profile_id;
        ctx.active_profile_id = Some(*profile_id);
        self.profile_repo.save_game_profile_context(&ctx)?;

        Ok(())
    }

    pub fn duplicate_profile(
        &self,
        profile_id: &ProfileId,
        new_name: &str,
    ) -> AppResult<ProfileSummaryDto> {
        let original = self
            .profile_repo
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;

        let new_summary = self.create_profile(
            &original.game_installation_id,
            new_name,
            original.description.as_deref(),
        )?;
        let new_pid = ProfileId::from_str(&new_summary.id)
            .map_err(|e| AppError::internal("Invalid ProfileId", e.to_string()))?;

        let comps = self.deployment_repo.list_profile_components(profile_id)?;
        for mut comp in comps {
            comp.id = manager_core::ids::ProfileComponentId::new();
            comp.profile_id = new_pid;
            self.deployment_repo.save_profile_component(&comp)?;
        }

        let count = self
            .deployment_repo
            .list_profile_components(&new_pid)?
            .len();
        let created = self.profile_repo.get_profile(&new_pid)?.unwrap();

        Ok(Self::profile_to_dto(&created, count))
    }

    pub fn delete_profile(&self, profile_id: &ProfileId) -> AppResult<()> {
        let mut profile = self
            .profile_repo
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;

        profile.state = ProfileState::Archived;
        profile.updated_at = Utc::now();
        self.profile_repo.save_profile(&profile)
    }

    fn profile_to_dto(p: &Profile, mod_count: usize) -> ProfileSummaryDto {
        let state_str = match p.state {
            ProfileState::Active => "active",
            ProfileState::Archived => "archived",
            ProfileState::Corrupted => "corrupted",
        };

        ProfileSummaryDto {
            id: p.id.to_string(),
            game_installation_id: p.game_installation_id.to_string(),
            name: p.name.clone(),
            description: p.description.clone(),
            revision: p.revision,
            mod_count,
            created_at: p.created_at.to_rfc3339(),
            updated_at: p.updated_at.to_rfc3339(),
            state: state_str.to_string(),
        }
    }
}
