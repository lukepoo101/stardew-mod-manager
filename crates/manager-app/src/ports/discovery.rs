use crate::error::AppResult;
use manager_core::game::{GameInspection, Storefront};
use std::path::{Path, PathBuf};

pub trait GameDiscoveryPort: Send + Sync {
    fn discover(&self) -> Vec<(PathBuf, Storefront)>;
}

pub trait GameInstallationInspectorPort: Send + Sync {
    fn inspect(&self, path: &Path, storefront: Storefront) -> AppResult<GameInspection>;
}
