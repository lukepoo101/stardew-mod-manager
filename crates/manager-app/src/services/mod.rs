pub mod bootstrap;
pub mod diagnostics;
pub mod games;
pub mod health;
pub mod launch;
pub mod mods;
pub mod operation_compatibility;
pub mod operation_lifecycle;
pub mod operation_recovery;
pub mod operations;
pub mod packages;
pub mod profiles;
pub mod resources;
pub mod runtime_observer;
pub mod smapi;
pub mod toggle;
pub mod troubleshoot;

pub use bootstrap::BootstrapService;
pub use diagnostics::{DiagnosticsService, HostEnvironment};
pub use games::GamesService;
pub use health::HealthService;
pub use launch::LaunchService;
pub use mods::ModsService;
pub use operation_lifecycle::OperationLifecycle;
pub use operations::OperationsService;
pub use packages::PackagesService;
pub use profiles::ProfilesService;
pub use resources::{ResourceClaim, ResourceCoordinator, ResourceLease};
pub use runtime_observer::RuntimeObserver;
pub use smapi::SmapiService;
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
    pub troubleshoot: Arc<TroubleshootService>,
}
