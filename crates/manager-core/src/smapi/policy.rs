use crate::smapi::model::{SmapiPlatformPolicy, SmapiReleaseInfo, SmapiReleasePolicy};
use std::collections::HashMap;

pub const PINNED_SMAPI_VERSION: &str = "4.1.10";
pub const PINNED_SMAPI_URL: &str =
    "https://github.com/Pathoschild/SMAPI/releases/download/4.1.10/SMAPI-4.1.10-installer.zip";
pub const PINNED_SMAPI_SHA256: &str =
    "8c127148a76c890e485aea73910189dc41c80f822c2a0a5e3b0e762b4ee3a93e";
pub const PINNED_SMAPI_TAG: &str = "4.1.10";
pub const PINNED_SMAPI_COMMIT: &str = "fd73446090cd71f4948f34ba8c428e45aa0a3ebf";
pub const PINNED_SMAPI_GAME_VERSION: &str = "1.6.9+";
pub const PINNED_INSTALLER_INTERNAL_PATH: &str =
    "SMAPI 4.1.10 installer/internal/linux/SMAPI.Installer";
pub const SMAPI_EXECUTABLE_NAME: &str = "StardewModdingAPI";
pub const SMAPI_LAUNCHER_SCRIPT_NAME: &str = "StardewValley";

const WINDOWS_INSTALLER_PATH: &str = "SMAPI 4.1.10 installer/internal/windows/SMAPI.Installer.exe";
const MACOS_INSTALLER_PATH: &str = "SMAPI 4.1.10 installer/internal/macOS/SMAPI.Installer";

#[cfg(target_os = "windows")]
const CURRENT_INSTALLER_PATH: &str = WINDOWS_INSTALLER_PATH;
#[cfg(not(target_os = "windows"))]
#[cfg(target_os = "macos")]
const CURRENT_INSTALLER_PATH: &str = MACOS_INSTALLER_PATH;
#[cfg(target_os = "linux")]
const CURRENT_INSTALLER_PATH: &str = PINNED_INSTALLER_INTERNAL_PATH;

#[cfg(target_os = "windows")]
const CURRENT_LAUNCHER_PATH: &str = "StardewModdingAPI.exe";
#[cfg(not(target_os = "windows"))]
const CURRENT_LAUNCHER_PATH: &str = SMAPI_EXECUTABLE_NAME;

pub fn get_pinned_smapi_release() -> SmapiReleaseInfo {
    SmapiReleaseInfo {
        version: PINNED_SMAPI_VERSION.to_string(),
        asset_url: PINNED_SMAPI_URL.to_string(),
        sha256: PINNED_SMAPI_SHA256.to_string(),
        tag: PINNED_SMAPI_TAG.to_string(),
        commit: PINNED_SMAPI_COMMIT.to_string(),
        supported_game_version: PINNED_SMAPI_GAME_VERSION.to_string(),
        installer_exec_path: CURRENT_INSTALLER_PATH.to_string(),
        launcher_exec_path: CURRENT_LAUNCHER_PATH.to_string(),
    }
}

pub fn default_release_policy() -> SmapiReleasePolicy {
    let mut platforms = HashMap::new();
    platforms.insert(
        "linux".to_string(),
        SmapiPlatformPolicy {
            url: PINNED_SMAPI_URL.to_string(),
            sha256: PINNED_SMAPI_SHA256.to_string(),
            installer_path: PINNED_INSTALLER_INTERNAL_PATH.to_string(),
            launcher_path: SMAPI_EXECUTABLE_NAME.to_string(),
        },
    );
    platforms.insert(
        "windows".to_string(),
        SmapiPlatformPolicy {
            url: PINNED_SMAPI_URL.to_string(),
            sha256: PINNED_SMAPI_SHA256.to_string(),
            installer_path: WINDOWS_INSTALLER_PATH.to_string(),
            launcher_path: "StardewModdingAPI.exe".to_string(),
        },
    );
    platforms.insert(
        "macos".to_string(),
        SmapiPlatformPolicy {
            url: PINNED_SMAPI_URL.to_string(),
            sha256: PINNED_SMAPI_SHA256.to_string(),
            installer_path: MACOS_INSTALLER_PATH.to_string(),
            launcher_path: SMAPI_EXECUTABLE_NAME.to_string(),
        },
    );

    SmapiReleasePolicy {
        schema_version: 1,
        tested_version: PINNED_SMAPI_VERSION.to_string(),
        tag: PINNED_SMAPI_TAG.to_string(),
        commit: PINNED_SMAPI_COMMIT.to_string(),
        supported_game_versions: vec![PINNED_SMAPI_GAME_VERSION.to_string()],
        platforms,
    }
}
