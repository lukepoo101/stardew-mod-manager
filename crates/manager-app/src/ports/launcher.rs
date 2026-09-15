use crate::error::AppResult;
use manager_core::launch::LaunchSpec;

/// Narrow game launching capability for starting and monitoring Stardew Valley sessions.
pub trait GameLauncherPort: Send + Sync {
    fn launch_game(&self, spec: &LaunchSpec) -> AppResult<u32>;
    fn is_game_running(&self, pid: Option<u32>) -> bool;
    fn terminate_game(&self, pid: Option<u32>) -> AppResult<()>;
}
