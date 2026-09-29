//! The Windows process image names the manager recognises.

use crate::platform::shared::layouts::WINDOWS_LAYOUT;

/// Names a running Stardew Valley or SMAPI process may appear under.
///
/// Both the vanilla executable and the SMAPI launcher have to be recognised:
/// SMAPI on Windows starts the game as a child, so either image can be the one
/// holding the installation's files open.
pub fn expected_process_images() -> Vec<String> {
    let mut images: Vec<String> = WINDOWS_LAYOUT
        .launcher_names
        .iter()
        .chain(WINDOWS_LAYOUT.smapi_launcher_names.iter())
        .map(|name| (*name).to_string())
        .collect();
    for candidate in ["Stardew Valley.exe", "StardewModdingAPI.exe"] {
        if !images.iter().any(|image| image == candidate) {
            images.push(candidate.to_string());
        }
    }
    images
}

/// Whether an image file name is the Windows game or SMAPI launcher.
pub fn is_game_process_image(file_name: &str) -> bool {
    expected_process_images()
        .iter()
        .any(|expected| expected.eq_ignore_ascii_case(file_name))
}
