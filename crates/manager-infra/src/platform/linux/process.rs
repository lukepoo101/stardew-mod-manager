//! The Linux process image names the manager recognises.

use crate::platform::shared::layouts::LINUX_LAYOUT;

/// Names a running Stardew Valley or SMAPI process may appear under.
pub fn expected_process_images() -> Vec<String> {
    let mut images: Vec<String> = LINUX_LAYOUT
        .launcher_names
        .iter()
        .chain(LINUX_LAYOUT.smapi_launcher_names.iter())
        .map(|name| (*name).to_string())
        .collect();
    for candidate in ["StardewModdingAPI", "StardewValley"] {
        if !images.iter().any(|image| image == candidate) {
            images.push(candidate.to_string());
        }
    }
    images
}
