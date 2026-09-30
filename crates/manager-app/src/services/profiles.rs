use crate::api::dto::ProfileSummaryDto;
use crate::error::{AppError, AppResult};
use crate::ports::repositories::{
    AtomicMutationStore, DeploymentRepository, ProfileCreateCommit, ProfileRepository,
};
use chrono::Utc;
use manager_core::ids::{GameInstallationId, OperationId, ProfileId};
use manager_core::operation::OperationEffect;
use manager_core::profile::{GameProfileContext, Profile, ProfileState};
use std::sync::Arc;

pub const MAX_PROFILE_NAME_CHARS: usize = 60;
pub const MAX_PROFILE_DESCRIPTION_CHARS: usize = 500;

/// A trimmed display name, or why it cannot be used. The name is only a label:
/// a profile's identity and folders come from its id and never change.
pub fn validate_profile_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::validation(
            "EMPTY_PROFILE_NAME",
            "Profile name cannot be empty",
        ));
    }
    if name.chars().count() > MAX_PROFILE_NAME_CHARS {
        return Err(AppError::validation(
            "PROFILE_NAME_TOO_LONG",
            format!("Profile names can be at most {MAX_PROFILE_NAME_CHARS} characters"),
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(AppError::validation(
            "PROFILE_NAME_INVALID",
            "Profile names cannot contain control characters",
        ));
    }
    Ok(name.to_string())
}

fn validate_description(description: Option<&str>) -> AppResult<Option<String>> {
    let Some(text) = description.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    if text.chars().count() > MAX_PROFILE_DESCRIPTION_CHARS {
        return Err(AppError::validation(
            "PROFILE_DESCRIPTION_TOO_LONG",
            format!("Descriptions can be at most {MAX_PROFILE_DESCRIPTION_CHARS} characters"),
        ));
    }
    Ok(Some(text.to_string()))
}

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

    /// Lists the profiles selectable for play. Archived profiles are reachable
    /// through [`Self::list_archived_profiles`] and have to be restored first.
    pub fn list_profiles(&self, game_id: &GameInstallationId) -> AppResult<Vec<ProfileSummaryDto>> {
        self.list_profiles_in_state(game_id, |state| state != ProfileState::Archived)
    }

    pub fn list_archived_profiles(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Vec<ProfileSummaryDto>> {
        self.list_profiles_in_state(game_id, |state| state == ProfileState::Archived)
    }

    fn list_profiles_in_state(
        &self,
        game_id: &GameInstallationId,
        keep: impl Fn(ProfileState) -> bool,
    ) -> AppResult<Vec<ProfileSummaryDto>> {
        let profiles = self.profile_repo.list_profiles(game_id)?;
        let mut dtos = Vec::new();

        for p in profiles.into_iter().filter(|p| keep(p.state)) {
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
        let trimmed_name = validate_profile_name(name)?;
        let trimmed_name = trimmed_name.as_str();
        let description = validate_description(description)?;

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
            description,
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

    /// Renames a profile or changes its description.
    ///
    /// Only the label changes: the id, folders, revision and history stay as
    /// they are, so previews prepared against the profile remain valid.
    pub fn update_profile_details(
        &self,
        profile_id: &ProfileId,
        name: &str,
        description: Option<&str>,
    ) -> AppResult<ProfileSummaryDto> {
        let name = validate_profile_name(name)?;
        let description = validate_description(description)?;
        let mut profile = self.profile_repo.get_profile(profile_id)?.ok_or_else(|| {
            AppError::validation("PROFILE_NOT_FOUND", "That profile does not exist")
        })?;
        if self
            .profile_repo
            .list_profiles(&profile.game_installation_id)?
            .iter()
            .any(|other| other.id != profile.id && other.name.eq_ignore_ascii_case(&name))
        {
            return Err(AppError::validation(
                "DUPLICATE_PROFILE_NAME",
                format!("A profile named '{name}' already exists"),
            ));
        }
        profile.name = name;
        profile.description = description;
        profile.updated_at = Utc::now();
        self.profile_repo.update_profile_details(
            &profile.id,
            &profile.name,
            profile.description.as_deref(),
            profile.updated_at,
        )?;
        let mod_count = self
            .deployment_repo
            .list_profile_components(&profile.id)?
            .len();
        Ok(Self::profile_to_dto(&profile, mod_count))
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

        match profile.state {
            ProfileState::Active => {}
            ProfileState::Archived => {
                return Err(AppError::validation(
                    "PROFILE_ARCHIVED",
                    "Archived profiles cannot be activated until they are restored",
                ))
            }
            ProfileState::Corrupted => {
                return Err(AppError::validation(
                    "PROFILE_CORRUPTED",
                    "A corrupted profile cannot be activated",
                ))
            }
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

    /// Archives a profile. Deployed files are retained so the profile can be restored;
    /// the active and default profiles of a game cannot be archived.
    pub fn archive_profile(&self, profile_id: &ProfileId) -> AppResult<()> {
        let mut profile = self
            .profile_repo
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;

        if profile.state == ProfileState::Archived {
            return Ok(());
        }

        if let Some(ctx) = self
            .profile_repo
            .get_game_profile_context(&profile.game_installation_id)?
        {
            if ctx.active_profile_id == Some(*profile_id) {
                return Err(AppError::validation(
                    "PROFILE_IS_ACTIVE",
                    "Switch to another profile before archiving this one",
                ));
            }
            if ctx.default_profile_id == Some(*profile_id) {
                return Err(AppError::validation(
                    "PROFILE_IS_DEFAULT",
                    "The default profile cannot be archived",
                ));
            }
        }

        profile.state = ProfileState::Archived;
        profile.revision = profile.revision.saturating_add(1);
        profile.updated_at = Utc::now();
        self.profile_repo.save_profile(&profile)
    }

    /// Returns an archived profile to the selectable set.
    pub fn restore_profile(&self, profile_id: &ProfileId) -> AppResult<()> {
        let mut profile = self
            .profile_repo
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;

        if profile.state != ProfileState::Archived {
            return Err(AppError::validation(
                "PROFILE_NOT_ARCHIVED",
                "Only archived profiles can be restored",
            ));
        }

        profile.state = ProfileState::Active;
        profile.revision = profile.revision.saturating_add(1);
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
