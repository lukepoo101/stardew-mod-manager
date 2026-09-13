use manager_core::use_cases::CoreUseCases;
use manager_infra::archive::PendingInspectionStore;
use manager_infra::db::SqliteStateRepository;
use manager_infra::launcher::DetachedGameLauncher;
use manager_infra::lock::FileInstanceLock;
use manager_infra::log_reader::SmapiSessionLogReader;
use manager_infra::package_store::FilesystemPackageStore;
use manager_infra::paths::AppPaths;
use manager_infra::smapi_adapter::ProcessSmapiInstaller;
use std::sync::Arc;

pub type AppUseCases = CoreUseCases<
    SqliteStateRepository,
    FilesystemPackageStore,
    ProcessSmapiInstaller,
    DetachedGameLauncher,
    SmapiSessionLogReader,
    FileInstanceLock,
>;

pub struct AppState {
    pub paths: AppPaths,
    pub use_cases: Arc<AppUseCases>,
    pub pending_plans: PendingInspectionStore,
    pub recovery_error: std::sync::Mutex<Option<String>>,
}

impl AppState {
    pub fn validate_setup(&self, id: &str) -> Result<manager_core::domain::Setup, String> {
        manager_core::install::validate_relative_path(id)?;
        if id.contains('/') {
            return Err("Invalid setup ID".into());
        }
        let setup = manager_core::ports::StateRepository::get_setup(&self.use_cases.repo, id)?
            .ok_or("Setup not found")?;
        // Reject symlinked managed ancestors, including the Mods destination.
        for path in [
            self.paths.mods_dir(id),
            self.paths.staging_dir(id, "inspect"),
            self.paths.recovery_dir(id, "remove"),
        ] {
            for parent in path.ancestors() {
                match std::fs::symlink_metadata(parent) {
                    Ok(m) if m.file_type().is_symlink() => {
                        return Err("Managed paths must not contain symlinks".into())
                    }
                    Ok(_) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        Ok(setup)
    }

    pub fn new() -> Result<Self, String> {
        Self::new_with_paths(AppPaths::from_env_or_default())
    }

    pub fn new_with_paths(paths: AppPaths) -> Result<Self, String> {
        Self::new_with_expected_smapi_hash(paths, None)
    }

    pub fn new_with_expected_smapi_hash(
        paths: AppPaths,
        expected_smapi_sha256: Option<&str>,
    ) -> Result<Self, String> {
        paths
            .ensure_directories()
            .map_err(|e| format!("Failed to initialize app paths: {}", e))?;

        let repo = SqliteStateRepository::new(paths.state_db_path())?;
        let package_store = FilesystemPackageStore::new(paths.packages_dir());
        let smapi_installer = match expected_smapi_sha256 {
            Some(hash) => {
                ProcessSmapiInstaller::new_with_expected_hash(paths.smapi_cache_dir(), hash)
            }
            None => ProcessSmapiInstaller::new(paths.smapi_cache_dir()),
        };
        let launcher = if expected_smapi_sha256.is_some() {
            DetachedGameLauncher::isolated()
        } else {
            DetachedGameLauncher::new()
        };
        let log_reader = SmapiSessionLogReader::new(None);
        let lock = FileInstanceLock::new(paths.lock_file_path());

        let use_cases = Arc::new(CoreUseCases::new(
            repo,
            package_store,
            smapi_installer,
            launcher,
            log_reader,
            lock,
        ));

        // On startup: run idempotent crash recovery
        let paths_clone = paths.clone();
        let recovery_error = use_cases
            .recover_operations_with_resolver(move |setup_id| {
                (
                    paths_clone.mods_dir(setup_id),
                    paths_clone.staging_dir(setup_id, "inspect"),
                    paths_clone.recovery_dir(setup_id, "remove"),
                )
            })
            .err();

        let pending_plans = PendingInspectionStore::new();

        Ok(Self {
            paths,
            use_cases,
            pending_plans,
            recovery_error: std::sync::Mutex::new(recovery_error),
        })
    }
}
