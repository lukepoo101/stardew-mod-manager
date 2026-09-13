#![allow(clippy::result_large_err)]

pub mod api;
pub mod error;
pub mod ports;
pub mod queries;
pub mod services;

pub use api::*;
pub use error::{AppError, AppErrorCategory, AppResult, Recoverability};
pub use ports::*;
pub use queries::{ModsQueries, ProfileQueries};
pub use services::{
    AppServices, BootstrapService, DiagnosticsService, GamesService, HealthService, LaunchService,
    ModsService, OperationsService, PackagesService, ProfilesService, SmapiService,
};
