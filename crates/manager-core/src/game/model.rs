use crate::ids::GameInstallationId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatingSystem {
    Linux,
    Windows,
    MacOS,
}

impl OperatingSystem {
    /// The stable lowercase key used by the SMAPI release policy, the
    /// persistence layer and the IPC contract.
    pub fn as_key(&self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::MacOS => "macos",
        }
    }

    /// The operating system of the running process.
    pub fn host() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOS
        } else {
            Self::Linux
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Storefront {
    Steam,
    Gog,
    Manual,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementMode {
    Managed,
    ExternalUnmanaged,
}

/// Persistent identity and metadata for a recognized Stardew Valley installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameInstallation {
    pub id: GameInstallationId,
    pub canonical_root: PathBuf,
    pub operating_system: OperatingSystem,
    pub storefront: Storefront,
    pub management_mode: ManagementMode,
    pub created_at: DateTime<Utc>,
}
