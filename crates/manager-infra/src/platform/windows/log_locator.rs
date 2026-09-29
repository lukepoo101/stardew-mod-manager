//! Where SMAPI writes its session log on Windows.
//!
//! SMAPI resolves the game's data directory through the .NET special-folder
//! rules, which is %APPDATA%\StardewValley on Windows. Roaming is correct here:
//! that is where the game and SMAPI both write.

use manager_app::ports::host::SessionLogLocatorPort;
use std::path::PathBuf;

pub const LOG_FILE_NAME: &str = "SMAPI-latest.txt";

#[derive(Debug, Clone, Default)]
pub struct WindowsSmapiLogLocator;

impl WindowsSmapiLogLocator {
    pub fn new() -> Self {
        Self
    }

    fn roaming_app_data() -> Option<PathBuf> {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(|profile| PathBuf::from(profile).join("AppData").join("Roaming"))
            })
    }
}

impl SessionLogLocatorPort for WindowsSmapiLogLocator {
    fn candidate_log_paths(&self) -> Vec<PathBuf> {
        Self::roaming_app_data()
            .map(|app_data| {
                vec![app_data
                    .join("StardewValley")
                    .join("ErrorLogs")
                    .join(LOG_FILE_NAME)]
            })
            .unwrap_or_default()
    }
}
