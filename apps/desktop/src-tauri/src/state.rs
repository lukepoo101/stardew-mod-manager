use manager_app::queries::{ModsQueries, ProfileQueries};
use manager_app::services::AppServices;
use manager_infra::archive::{SafeZipExtractor, StagedContentVerifier};
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::discovery::{LinuxGameInspector, SteamGameDiscovery};
use manager_infra::http::ReqwestDownloader;
use manager_infra::launcher::DetachedGameLauncher;
use manager_infra::lock::FileInstanceLock;
use manager_infra::log_reader::SmapiSessionLogReader;
use manager_infra::package_store::FilesystemPackageStore;
use manager_infra::paths::AppPaths;
use manager_infra::smapi_adapter::ProcessSmapiInstaller;
use std::sync::Arc;

pub struct AppState {
    pub paths: AppPaths,
    pub services: AppServices,
    pub mods_queries: Arc<ModsQueries>,
    pub profile_queries: Arc<ProfileQueries>,
    pub repo: Arc<SqliteStateRepository>,
}
impl AppState {
    pub fn new() -> Result<Self, String> {
        Self::new_with_paths(AppPaths::from_env_or_default()?)
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

        let repo = Arc::new(SqliteStateRepository::new(paths.state_db_path())?);
        let artifact_store = Arc::new(FilesystemPackageStore::new(paths.packages_dir()));
        let smapi_installer = Arc::new(match expected_smapi_sha256 {
            Some(hash) => {
                ProcessSmapiInstaller::new_with_expected_hash(paths.smapi_cache_dir(), hash)
            }
            None => ProcessSmapiInstaller::new(paths.smapi_cache_dir()),
        });
        let launcher = Arc::new(if expected_smapi_sha256.is_some() {
            DetachedGameLauncher::isolated()
        } else {
            DetachedGameLauncher::new()
        });
        let log_reader = Arc::new(SmapiSessionLogReader::new(None));
        let lock = Arc::new(FileInstanceLock::new(paths.lock_file_path()));
        let discovery = Arc::new(SteamGameDiscovery::new());
        let inspector = Arc::new(LinuxGameInspector::new());
        let downloader = Arc::new(ReqwestDownloader::new());
        let deployment = Arc::new(FilesystemDeploymentAdapter::new(paths.clone()));
        let staging = deployment.clone();
        let staging_verifier = Arc::new(StagedContentVerifier);
        let archive_inspector = Arc::new(SafeZipExtractor);

        let bootstrap_service = Arc::new(manager_app::services::BootstrapService::new(
            repo.clone(),
            repo.clone(),
            repo.clone(),
            "0.1.0",
        ));

        let games_service = Arc::new(manager_app::services::GamesService::new(
            repo.clone(),
            repo.clone(),
            repo.clone(),
            discovery,
            inspector,
        ));

        let profiles_service = Arc::new(manager_app::services::ProfilesService::new(
            repo.clone(),
            repo.clone(),
            repo.clone(),
        ));

        let packages_service = Arc::new(manager_app::services::PackagesService::new(
            repo.clone(),
            artifact_store.clone(),
        ));

        let smapi_service = Arc::new(manager_app::services::SmapiService::new(
            repo.clone(),
            repo.clone(),
            smapi_installer.clone(),
            smapi_installer.clone(),
            downloader,
            paths.smapi_cache_dir(),
            repo.clone(),
            launcher.clone(),
            lock.clone(),
        ));

        let mods_service = Arc::new(manager_app::services::ModsService::new(
            packages_service.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            archive_inspector,
            staging.clone(),
            staging_verifier.clone(),
            deployment.clone(),
        ));

        let operations_service = Arc::new(manager_app::services::OperationsService::new(
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            deployment.clone(),
            staging,
            staging_verifier,
            launcher.clone(),
            lock.clone(),
            repo.clone(),
            smapi_installer.clone(),
            repo.clone(),
        ));

        let launch_service = Arc::new(manager_app::services::LaunchService::new(
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            launcher.clone(),
            deployment,
            log_reader.clone(),
            lock.clone(),
        ));

        let diagnostics_service = Arc::new(manager_app::services::DiagnosticsService::new(
            repo.clone(),
            log_reader.clone(),
        ));

        let health_service = Arc::new(manager_app::services::HealthService::new(
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
        ));

        let services = AppServices {
            bootstrap: bootstrap_service,
            games: games_service,
            profiles: profiles_service.clone(),
            packages: packages_service,
            mods: mods_service,
            operations: operations_service.clone(),
            smapi: smapi_service.clone(),
            launch: launch_service,
            diagnostics: diagnostics_service,
            health: health_service.clone(),
        };

        let mods_queries = Arc::new(ModsQueries::new(repo.clone(), repo.clone()));

        let profile_queries = Arc::new(ProfileQueries::new(
            profiles_service,
            health_service,
            smapi_service,
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
        ));

        // On startup: reconcile interrupted operations individually. A single
        // operation that needs a human never stops the application from opening;
        // only a failure that prevents recovery processing itself is fatal.
        operations_service
            .recover_on_startup()
            .map_err(|e| format!("Startup recovery could not run: {}", e))?;

        Ok(Self {
            paths,
            services,
            mods_queries,
            profile_queries,
            repo,
        })
    }
}
