//! Discovery and inspection adapters.
//!
//! The implementations now live under `platform`; this module keeps the stable
//! paths the rest of the crate and its tests already use.

pub use crate::platform::discovery::PlatformGameInspector;
#[cfg(target_os = "linux")]
pub use crate::platform::linux::inspector::LinuxGameInspector;
#[cfg(target_os = "linux")]
pub use crate::platform::linux::steam::LinuxSteamLocator;
pub use crate::platform::shared::deps::{assembly_version, game_version};
pub use crate::platform::shared::layouts::{layout_for, LINUX_LAYOUT, WINDOWS_LAYOUT};
pub use crate::platform::shared::vdf::{library_paths_from_vdf, parse_vdf, VdfObject, VdfValue};
pub use crate::platform::steam::{
    discover_installations_from_roots, discover_with_locator, manifest_confirms_game,
    SteamRootLocator, GAME_DIRECTORY_NAME, STEAMAPPS_DIRECTORY,
};
pub use crate::platform::steam_discovery::SteamGameDiscovery;
#[cfg(target_os = "windows")]
pub use crate::platform::windows::inspector::WindowsGameInspector;
#[cfg(target_os = "windows")]
pub use crate::platform::windows::steam::WindowsSteamLocator;
