pub mod dependency;
pub mod deployment;
pub mod domain;
pub mod game;
pub mod health;
pub mod ids;
pub mod install;
pub mod launch;
pub mod manifest;
pub mod operation;
pub mod package;
pub mod ports;
pub mod profile;
pub mod smapi;
pub mod use_cases;
pub mod version;

pub use dependency::{
    build_dependency_graph, evaluate_bundle_dependencies, evaluate_dependencies, DependencyEdge,
    DependencyEdgeType, DependencyFinding, DependencyGraph, DependencyNode, DependencyReport,
};
pub use deployment::{DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment};
pub use domain::*;
pub use game::{classify_game_support, GameInspection, SupportState, GAME_APP_ID};
pub use health::{Finding, FindingAction, FindingCategory, FindingSeverity};
pub use ids::{
    derive_uuid, hash_to_hex, AcquisitionId, ArtifactHash, DeploymentId, FindingId,
    GameInstallationId,
    LaunchSessionId, ModUniqueId, OperationId, PackageComponentId, ProfileComponentId, ProfileId,
};
pub use install::{
    validate_relative_path, ArchiveInspectionResult, ComponentManifest, InstallPlan,
    InventoryEntry, InventoryEntryType, RemovalPlan, MAX_COMPRESSED_BYTES, MAX_ENTRY_COUNT,
    MAX_UNCOMPRESSED_BYTES,
};
pub use launch::{
    LaunchMode, LaunchSession, LaunchSpec, ModVerificationEvidence, PreflightCheck, SessionState,
    SessionVerificationBaseline, SessionVerificationResult, VerificationResult,
};
pub use manifest::{parse_manifest, ContentPackFor, Manifest, ModDependency};
pub use operation::{
    is_valid_transition, AccessMode, Operation, OperationEffect, OperationKind, OperationResource,
    OperationState, OperationStep, OperationStepState, ResourceKind,
};
pub use package::{Acquisition, AcquisitionSource, PackageArtifact, PackageComponent};
pub use ports::*;
pub use profile::{AppContext, GameProfileContext, OnboardingDisposition, Profile, ProfileState};
pub use smapi::{
    default_release_policy, get_pinned_smapi_release, ManagedSmapiInstallation, SmapiObservation,
    SmapiPlatformPolicy, SmapiReleaseInfo, SmapiReleasePolicy, PINNED_INSTALLER_INTERNAL_PATH,
    PINNED_SMAPI_COMMIT, PINNED_SMAPI_GAME_VERSION, PINNED_SMAPI_SHA256, PINNED_SMAPI_TAG,
    PINNED_SMAPI_URL, PINNED_SMAPI_VERSION, SMAPI_EXECUTABLE_NAME, SMAPI_LAUNCHER_SCRIPT_NAME,
};
pub use use_cases::{uuid_v4, AppSnapshot, CoreUseCases};
pub use version::SmapiVersion;
