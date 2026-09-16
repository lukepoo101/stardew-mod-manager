pub mod inspection;
pub mod model;

pub use inspection::{classify_game_support, GameInspection, SupportState};
pub use model::{GameInstallation, ManagementMode, OperatingSystem, Storefront};

pub const GAME_APP_ID: &str = "413150";
