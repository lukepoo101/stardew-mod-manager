pub mod bootstrap;
pub mod diagnostics;
pub mod games;
pub mod health;
pub mod launch;
pub mod mods;
pub mod operations;
pub mod packages;
pub mod profiles;
pub mod smapi;

pub use bootstrap::BootstrapService;
pub use diagnostics::DiagnosticsService;
pub use games::GamesService;
pub use health::HealthService;
pub use launch::LaunchService;
pub use mods::ModsService;
pub use operations::OperationsService;
pub use packages::PackagesService;
pub use profiles::ProfilesService;
pub use smapi::SmapiService;

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
}
