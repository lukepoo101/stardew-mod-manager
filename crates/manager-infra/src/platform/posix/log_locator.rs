//! Where SMAPI writes its session log on Linux and macOS.
//!
//! SMAPI resolves the game's data directory through the .NET special-folder
//! rules, which honour XDG_CONFIG_HOME and otherwise fall back to
//! ~/.config/StardewValley on both platforms.

use manager_app::ports::host::SessionLogLocatorPort;
use std::path::PathBuf;

pub const LOG_FILE_NAME: &str = "SMAPI-latest.txt";

#[derive(Debug, Clone, Default)]
pub struct PosixSmapiLogLocator;

impl PosixSmapiLogLocator {
    pub fn new() -> Self {
        Self
    }

    fn config_home() -> Option<PathBuf> {
        if let Some(custom) = std::env::var_os("XDG_CONFIG_HOME") {
            let path = PathBuf::from(custom);
            if !path.as_os_str().is_empty() {
                return Some(path);
            }
        }
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| !home.as_os_str().is_empty())
            .map(|home| home.join(".config"))
    }
}

impl SessionLogLocatorPort for PosixSmapiLogLocator {
    fn candidate_log_paths(&self) -> Vec<PathBuf> {
        Self::config_home()
            .map(|config| {
                vec![config
                    .join("StardewValley")
                    .join("ErrorLogs")
                    .join(LOG_FILE_NAME)]
            })
            .unwrap_or_default()
    }
}
