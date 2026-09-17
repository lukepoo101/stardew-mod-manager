//! The per-platform installation layouts the manager recognises.

use crate::platform::shared::inspector::InstallationLayout;
use manager_core::game::OperatingSystem;

/// Native Linux installation, including the Steam Deck and other Flatpak
/// installs, which all ship the same file set.
pub static LINUX_LAYOUT: InstallationLayout = InstallationLayout {
    operating_system: OperatingSystem::Linux,
    launcher_names: &[
        "Stardew Valley",
        "StardewValley",
        "StardewValley.bin.x86_64",
    ],
    required_files: &["Stardew Valley.dll"],
    smapi_launcher_names: &["StardewModdingAPI", "StardewModdingAPI.bin.x86_64"],
    canonical_smapi_launcher: "StardewModdingAPI",
    canonical_vanilla_launcher: "StardewValley",
    foreign_layout_evidence:
        "Expected the native game launcher and Stardew Valley.dll in this folder",
};

/// Native Windows installation as shipped by Steam and GOG.
///
/// A Proton or Wine prefix presents the Windows file set but is a Linux
/// installation on disk; it is recognised here and rejected by the platform
/// guard rather than silently reported as a native Windows install.
pub static WINDOWS_LAYOUT: InstallationLayout = InstallationLayout {
    operating_system: OperatingSystem::Windows,
    launcher_names: &["Stardew Valley.exe"],
    required_files: &[
        "Stardew Valley.dll",
        "Stardew Valley.deps.json",
        "StardewValley.GameData.dll",
    ],
    smapi_launcher_names: &["StardewModdingAPI.exe"],
    canonical_smapi_launcher: "StardewModdingAPI.exe",
    canonical_vanilla_launcher: "Stardew Valley.exe",
    foreign_layout_evidence:
        "The Windows game launcher 'Stardew Valley.exe' was not found in this folder",
};

/// The layout for an operating system, when the manager knows one.
pub fn layout_for(operating_system: OperatingSystem) -> Option<&'static InstallationLayout> {
    match operating_system {
        OperatingSystem::Linux => Some(&LINUX_LAYOUT),
        OperatingSystem::Windows => Some(&WINDOWS_LAYOUT),
        OperatingSystem::MacOS => None,
    }
}
