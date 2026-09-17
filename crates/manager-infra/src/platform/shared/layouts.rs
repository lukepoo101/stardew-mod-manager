//! The per-platform installation layouts the manager recognises.

use crate::platform::shared::inspector::InstallationLayout;
use manager_core::game::OperatingSystem;

/// The POSIX installation layout, shared by Linux and macOS.
///
/// Both ship the same file set: the native launcher, Stardew Valley.dll, and
/// SMAPI's extension-less launcher. Steam Deck, Flatpak and macOS installs all
/// present this layout.
pub static POSIX_LAYOUT: InstallationLayout = InstallationLayout {
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
    foreign_operating_system: OperatingSystem::Windows,
};

/// The Linux name for the POSIX layout, kept because the platform vocabulary in
/// the rest of the codebase and in persisted state says "linux".
pub static LINUX_LAYOUT: InstallationLayout = POSIX_LAYOUT;

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
    foreign_operating_system: OperatingSystem::Linux,
};

/// The layout for an operating system the manager knows.
///
/// macOS reports the POSIX layout rather than one of its own: the file set is
/// the same, and the difference that matters - the Steam client locations - is
/// handled by that platform's discovery.
pub fn layout_for(operating_system: OperatingSystem) -> Option<&'static InstallationLayout> {
    match operating_system {
        OperatingSystem::Linux | OperatingSystem::MacOS => Some(&POSIX_LAYOUT),
        OperatingSystem::Windows => Some(&WINDOWS_LAYOUT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_the_manager_models_has_a_layout() {
        for operating_system in [
            OperatingSystem::Linux,
            OperatingSystem::Windows,
            OperatingSystem::MacOS,
        ] {
            assert!(
                layout_for(operating_system).is_some(),
                "{} needs an installation layout",
                operating_system.as_key()
            );
        }
    }

    #[test]
    fn windows_and_posix_do_not_share_a_launcher_name() {
        let windows = layout_for(OperatingSystem::Windows).unwrap();
        let posix = layout_for(OperatingSystem::Linux).unwrap();
        assert!(windows.canonical_vanilla_launcher.ends_with(".exe"));
        assert!(!posix.canonical_vanilla_launcher.ends_with(".exe"));
    }
}
