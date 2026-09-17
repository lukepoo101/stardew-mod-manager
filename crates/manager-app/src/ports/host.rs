//! Ports whose only implementations are the host platform adapters.
//!
//! These are declared in the application layer so services depend on the
//! capability, not on the platform that provides it.

use manager_core::path_semantics::PathSemantics;
use std::path::{Path, PathBuf};

/// Resolves the SMAPI session log for the current host and user.
///
/// SMAPI writes SMAPI-latest.txt into the game's data directory, which is
/// ~/.config/StardewValley on Linux, %APPDATA%\StardewValley on Windows and
/// ~/.config/StardewValley on macOS. The parser is shared; only this lookup is
/// platform-specific.
pub trait SessionLogLocatorPort: Send + Sync {
    /// Every location that could hold the current SMAPI log, most likely first.
    fn candidate_log_paths(&self) -> Vec<PathBuf>;

    /// The location the diagnostics surface should report.
    fn default_log_path(&self) -> PathBuf {
        self.candidate_log_paths()
            .into_iter()
            .next()
            .unwrap_or_else(|| PathBuf::from("SMAPI-latest.txt"))
    }
}

/// Host filesystem semantics: how paths compare and which names are legal.
pub trait HostPathSemanticsPort: Send + Sync {
    fn semantics(&self) -> &'static dyn PathSemantics;

    /// Whether two paths denote the same file on this host.
    fn paths_equivalent(&self, left: &Path, right: &Path) -> bool {
        self.semantics().paths_equivalent(left, right)
    }
}
