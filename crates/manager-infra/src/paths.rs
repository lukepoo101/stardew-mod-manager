use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    data_dir: PathBuf,
    cache_dir: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf, cache_dir: PathBuf) -> Self {
        Self {
            data_dir,
            cache_dir,
        }
    }

    pub fn from_env_or_default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let data_dir = match std::env::var("XDG_DATA_HOME") {
            Ok(p) => PathBuf::from(p).join("stardew-mod-manager"),
            Err(_) => PathBuf::from(&home).join(".local/share/stardew-mod-manager"),
        };
        let cache_dir = match std::env::var("XDG_CACHE_HOME") {
            Ok(p) => PathBuf::from(p).join("stardew-mod-manager"),
            Err(_) => PathBuf::from(&home).join(".cache/stardew-mod-manager"),
        };

        Self {
            data_dir,
            cache_dir,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn state_db_path(&self) -> PathBuf {
        self.data_dir.join("state.sqlite3")
    }

    pub fn lock_file_path(&self) -> PathBuf {
        self.data_dir.join(".instance.lock")
    }

    pub fn mods_dir(&self, setup_id: &str) -> PathBuf {
        self.data_dir.join("setups").join(setup_id).join("Mods")
    }

    pub fn staging_dir(&self, setup_id: &str, operation_id: &str) -> PathBuf {
        self.data_dir
            .join("setups")
            .join(setup_id)
            .join(".staging")
            .join(operation_id)
    }

    pub fn recovery_dir(&self, setup_id: &str, operation_id: &str) -> PathBuf {
        self.data_dir
            .join("setups")
            .join(setup_id)
            .join(".recovery")
            .join(operation_id)
    }

    pub fn packages_dir(&self) -> PathBuf {
        self.data_dir.join("packages")
    }

    pub fn package_path(&self, hash: &str) -> PathBuf {
        self.packages_dir().join(format!("{}.zip", hash))
    }

    pub fn smapi_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("smapi")
    }

    pub fn ensure_directories(&self) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        std::fs::create_dir_all(self.packages_dir())?;
        std::fs::create_dir_all(self.smapi_cache_dir())?;
        Ok(())
    }
}
