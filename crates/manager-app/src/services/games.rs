use crate::api::dto::{GameInspectionDto, GameInstallationSummaryDto};
use crate::error::{AppError, AppResult};
use crate::ports::discovery::{GameDiscoveryPort, GameInstallationInspectorPort};
use crate::ports::repositories::{
    AtomicMutationStore, GameInstallationRepository, ProfileCreateCommit, ProfileRepository,
};
use chrono::Utc;
use manager_core::game::{
    GameInspection, GameInstallation, ManagementMode, OperatingSystem, Storefront, SupportState,
};
use manager_core::ids::{GameInstallationId, ProfileId};
use manager_core::operation::OperationEffect;
use manager_core::profile::Profile;
use std::path::Path;
use std::sync::Arc;

pub struct GamesService {
    game_repo: Arc<dyn GameInstallationRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
    mutation_store: Arc<dyn AtomicMutationStore>,
    discovery: Arc<dyn GameDiscoveryPort>,
    inspector: Arc<dyn GameInstallationInspectorPort>,
}

impl GamesService {
    pub fn new(
        game_repo: Arc<dyn GameInstallationRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
        mutation_store: Arc<dyn AtomicMutationStore>,
        discovery: Arc<dyn GameDiscoveryPort>,
        inspector: Arc<dyn GameInstallationInspectorPort>,
    ) -> Self {
        Self {
            game_repo,
            profile_repo,
            mutation_store,
            discovery,
            inspector,
        }
    }

    pub fn discover_games(&self) -> AppResult<Vec<GameInspectionDto>> {
        let existing = self.game_repo.list_games()?;
        let mut discovered = self.discovery.discover();
        discovered.extend(
            existing
                .iter()
                .map(|game| (game.canonical_root.clone(), game.storefront)),
        );
        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();
        for (path, storefront) in discovered {
            let mut inspection = self.inspector.inspect(&path, storefront)?;
            if seen.insert(inspection.canonical_root.clone()) {
                Self::apply_registration(&mut inspection, &existing);
                results.push(Self::inspection_to_dto(&inspection));
            }
        }
        Ok(results)
    }

    fn apply_registration(inspection: &mut GameInspection, existing: &[GameInstallation]) {
        if let Some(game) = existing
            .iter()
            .find(|game| game.canonical_root == inspection.canonical_root)
        {
            inspection.installation_id = Some(game.id);
            inspection.storefront = game.storefront;
            // Registration cannot make a missing, unsupported or unwritable folder usable.
            if game.management_mode == ManagementMode::Managed
                && matches!(
                    inspection.support_state,
                    SupportState::SupportedFresh
                        | SupportState::ExistingModdedUnmanaged
                        | SupportState::SupportedManaged
                )
            {
                inspection.support_state = SupportState::SupportedManaged;
            }
        }
    }

    pub fn inspect_path(
        &self,
        path: &Path,
        storefront: Option<Storefront>,
    ) -> AppResult<GameInspectionDto> {
        let mut inspection = self
            .inspector
            .inspect(path, storefront.unwrap_or(Storefront::Manual))?;
        Self::apply_registration(&mut inspection, &self.game_repo.list_games()?);
        Ok(Self::inspection_to_dto(&inspection))
    }

    pub fn accept_game(
        &self,
        path: &Path,
        storefront: Storefront,
        mode: ManagementMode,
    ) -> AppResult<GameInstallationSummaryDto> {
        let existing = self.game_repo.list_games()?;
        let mut inspection = self.inspector.inspect(path, storefront)?;
        Self::apply_registration(&mut inspection, &existing);
        if !inspection.support_state.is_usable() && mode == ManagementMode::Managed {
            return Err(AppError::validation(
                "UNSUPPORTED_GAME_STATE",
                format!(
                    "Cannot manage game: {:?}. {}",
                    inspection.support_state,
                    inspection.evidence.join("; ")
                ),
            ));
        }
        let canonical_root = inspection.canonical_root.clone();
        let game_id =
            if let Some(found) = existing.iter().find(|g| g.canonical_root == canonical_root) {
                found.id
            } else {
                let id = GameInstallationId::new();
                let game = GameInstallation {
                    id,
                    canonical_root: canonical_root.clone(),
                    operating_system: inspection.operating_system,
                    storefront: inspection.storefront,
                    management_mode: mode,
                    created_at: Utc::now(),
                };
                self.game_repo.save_game(&game)?;
                id
            };

        // Create default "Main" profile if no profiles exist
        let profiles = self.profile_repo.list_profiles(&game_id)?;
        if profiles.is_empty() {
            let profile_id = ProfileId::new();
            let profile = Profile {
                id: profile_id,
                game_installation_id: game_id,
                name: "Main".to_string(),
                description: Some("Default modding profile".to_string()),
                revision: 1,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                state: manager_core::profile::ProfileState::Active,
            };

            let effect = OperationEffect {
                id: uuid::Uuid::new_v4().to_string(),
                operation_id: manager_core::ids::OperationId::new(),
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
                    profile,
                    set_as_active: true,
                    set_as_default: true,
                    effects: vec![effect],
                })?;
        }

        // Set as active game
        let mut app_ctx = self.game_repo.get_app_context()?;
        app_ctx.active_game_installation_id = Some(game_id);
        self.game_repo.save_app_context(&app_ctx)?;

        let saved = self.game_repo.get_game(&game_id)?.ok_or_else(|| {
            AppError::internal("Game not found after saving", game_id.to_string())
        })?;

        Ok(Self::game_to_dto(&saved))
    }

    pub fn list_games(&self) -> AppResult<Vec<GameInstallationSummaryDto>> {
        let games = self.game_repo.list_games()?;
        Ok(games.into_iter().map(|g| Self::game_to_dto(&g)).collect())
    }

    pub fn get_game(
        &self,
        id: &GameInstallationId,
    ) -> AppResult<Option<GameInstallationSummaryDto>> {
        let game = self.game_repo.get_game(id)?;
        Ok(game.map(|g| Self::game_to_dto(&g)))
    }

    pub fn set_active_game(&self, id: &GameInstallationId) -> AppResult<()> {
        let mut app_ctx = self.game_repo.get_app_context()?;
        app_ctx.active_game_installation_id = Some(*id);
        self.game_repo.save_app_context(&app_ctx)
    }

    fn inspection_to_dto(inspection: &GameInspection) -> GameInspectionDto {
        let state_str = match inspection.support_state {
            SupportState::SupportedFresh => "supported_fresh",
            SupportState::SupportedManaged => "supported_managed",
            SupportState::ExistingModdedUnmanaged => "existing_modded_unmanaged",
            SupportState::UnsupportedPlatform => "unsupported_platform",
            SupportState::InvalidGameDirectory => "invalid_game_directory",
            SupportState::Unreadable => "unreadable",
            SupportState::Unwritable => "unwritable",
            SupportState::Unknown => "unknown",
        };

        let sf_str = match inspection.storefront {
            Storefront::Steam => "steam",
            Storefront::Gog => "gog",
            Storefront::Manual => "manual",
            Storefront::Unknown => "unknown",
        };

        GameInspectionDto {
            candidate_path: inspection.canonical_root.to_string_lossy().to_string(),
            storefront: sf_str.to_string(),
            detected_version: inspection.observed_game_version.clone(),
            support_state: state_str.to_string(),
            is_usable: inspection.support_state.is_usable(),
            has_existing_smapi: inspection.has_existing_smapi,
            has_existing_mods: inspection.has_existing_mods,
            is_writable: inspection.is_writable,
            evidence: inspection.evidence.clone(),
        }
    }

    fn game_to_dto(g: &GameInstallation) -> GameInstallationSummaryDto {
        let os_str = match g.operating_system {
            OperatingSystem::Linux => "linux",
            OperatingSystem::Windows => "windows",
            OperatingSystem::MacOS => "macos",
        };
        let sf_str = match g.storefront {
            Storefront::Steam => "steam",
            Storefront::Gog => "gog",
            Storefront::Manual => "manual",
            Storefront::Unknown => "unknown",
        };
        let mode_str = match g.management_mode {
            ManagementMode::Managed => "managed",
            ManagementMode::ExternalUnmanaged => "external_unmanaged",
        };

        GameInstallationSummaryDto {
            id: g.id.to_string(),
            canonical_root: g.canonical_root.to_string_lossy().to_string(),
            operating_system: os_str.to_string(),
            storefront: sf_str.to_string(),
            management_mode: mode_str.to_string(),
            created_at: g.created_at.to_rfc3339(),
        }
    }
}
