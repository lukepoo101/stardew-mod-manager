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
    switch_guards: Option<(
        Arc<dyn crate::ports::repositories::OperationRepository>,
        Arc<dyn crate::ports::launcher::GameLauncherPort>,
    )>,
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
            switch_guards: None,
        }
    }

    /// Refuses to switch the active profile while the game is running or
    /// while either profile has a change that has not finished.
    pub fn with_switch_guards(
        mut self,
        operation_repo: Arc<dyn crate::ports::repositories::OperationRepository>,
        launcher: Arc<dyn crate::ports::launcher::GameLauncherPort>,
    ) -> Self {
        self.switch_guards = Some((operation_repo, launcher));
        self
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
        let default = self.default_profile_id(game_id)?;
        let mut dtos = Vec::new();

        for p in profiles.into_iter().filter(|p| keep(p.state)) {
            let mod_count = self.deployment_repo.list_profile_components(&p.id)?.len();
            let mut dto = Self::profile_to_dto(&p, mod_count);
            dto.is_default = default == Some(p.id);
            dtos.push(dto);
        }

        Ok(dtos)
    }

    pub fn get_profile(&self, id: &ProfileId) -> AppResult<Option<ProfileSummaryDto>> {
        if let Some(p) = self.profile_repo.get_profile(id)? {
            let mod_count = self.deployment_repo.list_profile_components(&p.id)?.len();
            let mut dto = Self::profile_to_dto(&p, mod_count);
            dto.is_default = self.default_profile_id(&p.game_installation_id)? == Some(p.id);
            Ok(Some(dto))
        } else {
            Ok(None)
        }
    }

    /// The explicitly active profile, or the default one when nothing is
    /// explicitly active. The default never replaces an explicit choice.
    pub fn get_active_profile(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Option<ProfileSummaryDto>> {
        match resolve_active_profile(self.profile_repo.as_ref(), game_id)? {
            Some(id) => self.get_profile(&id),
            None => Ok(None),
        }
    }

    fn default_profile_id(&self, game_id: &GameInstallationId) -> AppResult<Option<ProfileId>> {
        Ok(self
            .profile_repo
            .get_game_profile_context(game_id)?
            .and_then(|ctx| ctx.default_profile_id))
    }

    /// Marks a profile as the game's default, replacing any previous default,
    /// or clears the default when `profile_id` is `None`.
    ///
    /// The default is only a fallback for when no profile is explicitly
    /// active; setting it never changes the active profile.
    pub fn set_default_profile(
        &self,
        game_id: &GameInstallationId,
        profile_id: Option<&ProfileId>,
    ) -> AppResult<()> {
        if let Some(profile_id) = profile_id {
            let profile = self.profile_repo.get_profile(profile_id)?.ok_or_else(|| {
                AppError::validation("PROFILE_NOT_FOUND", "That profile does not exist")
            })?;
            if &profile.game_installation_id != game_id {
                return Err(AppError::validation(
                    "PROFILE_GAME_MISMATCH",
                    "Profile does not belong to the active game",
                ));
            }
            if profile.state != ProfileState::Active {
                return Err(AppError::validation(
                    "PROFILE_NOT_SELECTABLE",
                    "Only a profile that can be played can be the default",
                ));
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
        ctx.default_profile_id = profile_id.copied();
        self.profile_repo.save_game_profile_context(&ctx)
    }

    pub fn create_profile(
        &self,
        game_id: &GameInstallationId,
        name: &str,
        description: Option<&str>,
    ) -> AppResult<ProfileSummaryDto> {
        self.create(game_id, name, description, None)
    }

    /// Creates a profile that starts as a copy of `source`. The history
    /// records where it came from; the two profiles stay independent.
    pub fn create_profile_copy(
        &self,
        source: &Profile,
        name: &str,
        description: Option<&str>,
    ) -> AppResult<ProfileSummaryDto> {
        self.create(
            &source.game_installation_id,
            name,
            description,
            Some(source),
        )
    }

    fn create(
        &self,
        game_id: &GameInstallationId,
        name: &str,
        description: Option<&str>,
        copied_from: Option<&Profile>,
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

        let operation_id = OperationId::new();
        let mut effects = vec![OperationEffect {
            id: uuid::Uuid::new_v4().to_string(),
            operation_id,
            profile_id: Some(profile_id),
            entity_type: "profile".to_string(),
            entity_id: profile_id.to_string(),
            change_kind: "ProfileCreated".to_string(),
            before_json: None,
            after_json: serde_json::to_string(&profile).ok(),
            occurred_at: Utc::now(),
        }];
        if let Some(source) = copied_from {
            // Lineage for diagnostics only; nothing links the profiles.
            effects.push(OperationEffect {
                id: uuid::Uuid::new_v4().to_string(),
                operation_id,
                profile_id: Some(profile_id),
                entity_type: "profile".to_string(),
                entity_id: source.id.to_string(),
                change_kind: "ProfileClonedFrom".to_string(),
                before_json: None,
                after_json: serde_json::to_string(&serde_json::json!({
                    "name": source.name,
                    "profile_id": source.id.to_string(),
                    "revision": source.revision,
                }))
                .ok(),
                occurred_at: Utc::now(),
            });
        }

        self.mutation_store
            .commit_profile_create(ProfileCreateCommit {
                profile: profile.clone(),
                set_as_active: false,
                set_as_default: false,
                effects,
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

        if let Some((operation_repo, launcher)) = &self.switch_guards {
            if launcher.is_game_running(None) {
                return Err(AppError::game_running(
                    "Close Stardew Valley before switching profiles, so the running game keeps the mods it started with",
                ));
            }
            // Drafts and prepared previews change nothing yet; only work that
            // is changing files, or needs recovery, holds the profiles.
            use manager_core::operation::OperationState as S;
            let busy = operation_repo
                .list_unresolved_operations()?
                .into_iter()
                .any(|op| {
                    matches!(
                        op.state,
                        S::Running
                            | S::Committing
                            | S::CancellationRequested
                            | S::Cancelling
                            | S::RollingBack
                            | S::RecoveryRequired
                    ) && op
                        .profile_id
                        .is_some_and(|p| p == *profile_id || Some(p) == ctx.active_profile_id)
                });
            if busy {
                return Err(AppError::validation(
                    "PROFILE_SWITCH_BLOCKED",
                    "A change to this or the current profile has not finished. Finish or cancel it on the Activity page, then switch.",
                ));
            }
        }

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
            is_default: false,
        }
    }
}

/// The profile a game should use: the explicitly active one, or the default
/// when nothing is explicitly active (or the active one is no longer
/// playable). Returns `None` when neither is usable.
pub fn resolve_active_profile(
    profile_repo: &dyn ProfileRepository,
    game_id: &GameInstallationId,
) -> AppResult<Option<ProfileId>> {
    let Some(ctx) = profile_repo.get_game_profile_context(game_id)? else {
        return Ok(None);
    };
    if let Some(active) = ctx.active_profile_id {
        return Ok(Some(active));
    }
    match ctx.default_profile_id {
        Some(default) => Ok(profile_repo
            .get_profile(&default)?
            .filter(|p| p.state == ProfileState::Active)
            .map(|p| p.id)),
        None => Ok(None),
    }
}
