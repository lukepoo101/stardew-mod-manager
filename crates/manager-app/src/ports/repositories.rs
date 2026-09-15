use crate::error::AppResult;
use manager_core::deployment::{ProfileComponent, ProfileDeployment};
use manager_core::game::GameInstallation;
use manager_core::ids::{
    ArtifactHash, GameInstallationId, LaunchSessionId, OperationId, PackageComponentId,
    ProfileComponentId, ProfileId,
};
use manager_core::launch::LaunchSession;
use manager_core::operation::{
    Operation, OperationEffect, OperationResource, OperationState, OperationStep,
};
use manager_core::package::{Acquisition, PackageArtifact, PackageComponent};
use manager_core::profile::{AppContext, GameProfileContext, Profile};
use manager_core::smapi::ManagedSmapiInstallation;
use serde::{Deserialize, Serialize};

pub trait GameInstallationRepository: Send + Sync {
    fn save_game(&self, game: &GameInstallation) -> AppResult<()>;
    fn get_game(&self, id: &GameInstallationId) -> AppResult<Option<GameInstallation>>;
    fn list_games(&self) -> AppResult<Vec<GameInstallation>>;
    fn get_app_context(&self) -> AppResult<AppContext>;
    fn save_app_context(&self, ctx: &AppContext) -> AppResult<()>;
}

pub trait ProfileRepository: Send + Sync {
    fn save_profile(&self, profile: &Profile) -> AppResult<()>;
    fn get_profile(&self, id: &ProfileId) -> AppResult<Option<Profile>>;
    fn list_profiles(&self, game_id: &GameInstallationId) -> AppResult<Vec<Profile>>;
    fn get_game_profile_context(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Option<GameProfileContext>>;
    fn save_game_profile_context(&self, ctx: &GameProfileContext) -> AppResult<()>;
}

pub trait PackageCatalogRepository: Send + Sync {
    fn save_artifact(&self, artifact: &PackageArtifact) -> AppResult<()>;
    fn get_artifact(&self, hash: &ArtifactHash) -> AppResult<Option<PackageArtifact>>;
    fn list_artifacts(&self) -> AppResult<Vec<PackageArtifact>>;

    fn save_acquisition(&self, acquisition: &Acquisition) -> AppResult<()>;
    fn get_acquisitions_for_artifact(
        &self,
        artifact_hash: &ArtifactHash,
    ) -> AppResult<Vec<Acquisition>>;

    fn save_package_component(&self, comp: &PackageComponent) -> AppResult<()>;
    fn get_package_component(&self, id: &PackageComponentId)
        -> AppResult<Option<PackageComponent>>;
    fn list_components_for_artifact(
        &self,
        artifact_hash: &ArtifactHash,
    ) -> AppResult<Vec<PackageComponent>>;
}

pub trait DeploymentRepository: Send + Sync {
    fn save_deployment(&self, deployment: &ProfileDeployment) -> AppResult<()>;
    fn get_deployment(
        &self,
        id: &manager_core::ids::DeploymentId,
    ) -> AppResult<Option<ProfileDeployment>>;
    fn list_deployments_for_profile(
        &self,
        profile_id: &ProfileId,
    ) -> AppResult<Vec<ProfileDeployment>>;

    fn save_profile_component(&self, comp: &ProfileComponent) -> AppResult<()>;
    fn get_profile_component(&self, id: &ProfileComponentId)
        -> AppResult<Option<ProfileComponent>>;
    fn list_profile_components(&self, profile_id: &ProfileId) -> AppResult<Vec<ProfileComponent>>;
    fn delete_profile_component(&self, id: &ProfileComponentId) -> AppResult<()>;
}

pub trait OperationRepository: Send + Sync {
    fn save_operation(&self, op: &Operation) -> AppResult<()>;
    fn get_operation(&self, id: &OperationId) -> AppResult<Option<Operation>>;
    fn update_operation_state(
        &self,
        id: &OperationId,
        state: OperationState,
        error_code: Option<String>,
        error_json: Option<String>,
    ) -> AppResult<()>;
    fn list_unresolved_operations(&self) -> AppResult<Vec<Operation>>;
    fn list_recent_operations(&self, limit: usize) -> AppResult<Vec<Operation>>;
    fn list_operations_for_profile(&self, profile_id: &ProfileId) -> AppResult<Vec<Operation>>;

    fn save_operation_step(&self, step: &OperationStep) -> AppResult<()>;
    fn list_operation_steps(&self, op_id: &OperationId) -> AppResult<Vec<OperationStep>>;

    fn save_operation_resource(&self, res: &OperationResource) -> AppResult<()>;
    fn list_operation_resources(&self, op_id: &OperationId) -> AppResult<Vec<OperationResource>>;
    fn list_unresolved_resources_for_profile(
        &self,
        profile_id: &ProfileId,
    ) -> AppResult<Vec<OperationResource>>;

    fn save_operation_effect(&self, effect: &OperationEffect) -> AppResult<()>;
    fn list_operation_effects(&self, op_id: &OperationId) -> AppResult<Vec<OperationEffect>>;
    fn list_recent_effects_for_profile(
        &self,
        profile_id: &ProfileId,
        limit: usize,
    ) -> AppResult<Vec<OperationEffect>>;
}

pub trait LaunchSessionRepository: Send + Sync {
    fn save_launch_session(&self, session: &LaunchSession) -> AppResult<()>;
    fn get_launch_session(&self, id: &LaunchSessionId) -> AppResult<Option<LaunchSession>>;
    fn get_latest_launch_session(
        &self,
        profile_id: Option<&ProfileId>,
    ) -> AppResult<Option<LaunchSession>>;
    fn update_launch_session(&self, session: &LaunchSession) -> AppResult<()>;
    fn list_launch_sessions(
        &self,
        profile_id: &ProfileId,
        limit: usize,
    ) -> AppResult<Vec<LaunchSession>>;
}

pub trait SmapiRepository: Send + Sync {
    fn save_smapi_installation(&self, record: &ManagedSmapiInstallation) -> AppResult<()>;
    fn get_smapi_installation(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Option<ManagedSmapiInstallation>>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometryDto {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub is_maximized: bool,
}

pub trait PreferencesRepository: Send + Sync {
    fn get_window_geometry(&self) -> AppResult<Option<WindowGeometryDto>>;
    fn save_window_geometry(&self, geom: &WindowGeometryDto) -> AppResult<()>;
    fn get_preference(&self, key: &str) -> AppResult<Option<String>>;
    fn set_preference(&self, key: &str, value: &str) -> AppResult<()>;
}

/// Typed atomic mutation data payloads applied inside a single database transaction.
pub struct InstallCommit {
    pub operation_id: OperationId,
    pub profile_id: ProfileId,
    pub expected_profile_revision: u64,
    pub artifact: PackageArtifact,
    pub acquisition: Acquisition,
    pub package_components: Vec<PackageComponent>,
    pub deployment: ProfileDeployment,
    pub profile_components: Vec<ProfileComponent>,
    pub effects: Vec<OperationEffect>,
}

pub struct RemovalCommit {
    pub operation_id: OperationId,
    pub profile_id: ProfileId,
    pub expected_profile_revision: u64,
    pub deployment_id: manager_core::ids::DeploymentId,
    pub removed_profile_component_ids: Vec<ProfileComponentId>,
    pub effects: Vec<OperationEffect>,
}

pub struct ProfileCreateCommit {
    pub profile: Profile,
    pub set_as_active: bool,
    pub set_as_default: bool,
    pub effects: Vec<OperationEffect>,
}

pub trait AtomicMutationStore: Send + Sync {
    fn commit_install(&self, commit: InstallCommit) -> AppResult<()>;
    fn commit_removal(&self, commit: RemovalCommit) -> AppResult<()>;
    fn commit_profile_create(&self, commit: ProfileCreateCommit) -> AppResult<()>;
}
