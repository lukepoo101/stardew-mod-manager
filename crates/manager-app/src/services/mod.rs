pub mod annotations;
pub mod bootstrap;
pub mod bundle;
pub mod diagnostics;
pub mod dismissals;
pub mod experiments;
pub mod file_integrity;
pub mod freeze;
pub mod games;
pub mod health;
pub mod known_good;
pub mod launch;
pub mod mods;
pub mod operation_compatibility;
pub mod operation_lifecycle;
pub mod operation_recovery;
pub mod operations;
pub mod packages;
pub mod profile_deletion;
pub mod profiles;
pub mod reference_recipes;
pub mod reinstall;
pub mod resources;
pub mod restore_points;
pub mod runtime_observer;
pub mod saves;
pub mod smapi;
pub mod storage_cleanup;
pub mod toggle;
pub mod troubleshoot;

pub use annotations::ModAnnotations;
pub use bootstrap::BootstrapService;
pub use bundle::BundleService;
pub use diagnostics::{DiagnosticsService, HostEnvironment};
pub use dismissals::FindingDismissals;
pub use experiments::ProfileExperiments;
pub use file_integrity::FileIntegrityService;
pub use freeze::ProfileFreeze;
pub use games::GamesService;
pub use health::HealthService;
pub use known_good::KnownGood;
pub use launch::LaunchService;
pub use mods::ModsService;
pub use operation_lifecycle::OperationLifecycle;
pub use operations::OperationsService;
pub use packages::PackagesService;
pub use profile_deletion::ProfileDeletionService;
pub use profiles::ProfilesService;
pub use reference_recipes::ReferenceRecipes;
pub use reinstall::ReinstallService;
pub use resources::{ResourceClaim, ResourceCoordinator, ResourceLease};
pub use restore_points::RestorePoints;
pub use runtime_observer::RuntimeObserver;
pub use saves::SavesService;
pub use smapi::SmapiService;
pub use storage_cleanup::StorageCleanupService;
pub use toggle::ToggleService;
pub use troubleshoot::TroubleshootService;

use std::sync::Arc;

#[derive(Clone)]
pub struct AppServices {
    pub bootstrap: Arc<BootstrapService>,
    pub games: Arc<GamesService>,
    pub profiles: Arc<ProfilesService>,
    pub packages: Arc<PackagesService>,
    pub mods: Arc<ModsService>,
    pub operations: Arc<OperationsService>,
    pub smapi: Arc<SmapiService>,
    pub launch: Arc<LaunchService>,
    pub diagnostics: Arc<DiagnosticsService>,
    pub health: Arc<HealthService>,
    pub toggle: Arc<ToggleService>,
    pub bundle: Arc<BundleService>,
    pub storage: Arc<StorageCleanupService>,
    pub troubleshoot: Arc<TroubleshootService>,
    pub profile_deletion: Arc<ProfileDeletionService>,
    pub saves: Arc<SavesService>,
}
