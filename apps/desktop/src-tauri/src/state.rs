use manager_app::queries::{ModsQueries, ProfileQueries};
use manager_app::services::AppServices;
use manager_infra::archive::{SafeZipExtractor, StagedContentVerifier};
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::http::ReqwestDownloader;
use manager_infra::launcher::DetachedGameLauncher;
use manager_infra::lock::FileInstanceLock;
use manager_infra::log_reader::SmapiSessionLogReader;
use manager_infra::package_store::FilesystemPackageStore;
use manager_infra::paths::AppPaths;
use manager_infra::platform::HostPlatform;
use manager_infra::smapi_adapter::ProcessSmapiInstaller;
use std::sync::Arc;

pub struct AppState {
    pub paths: AppPaths,
    pub services: AppServices,
    pub mods_queries: Arc<ModsQueries>,
    pub profile_queries: Arc<ProfileQueries>,
    pub repo: Arc<SqliteStateRepository>,
    /// The adapters this build was composed with.
    ///
    /// Diagnostics reports the platform from here rather than guessing, so the
    /// answer always describes the code that is actually running.
    pub platform: Arc<HostPlatform>,
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

        let platform = Arc::new(HostPlatform::for_host());

        let repo = Arc::new(SqliteStateRepository::new(paths.state_db_path())?);
        let artifact_store = Arc::new(FilesystemPackageStore::new(paths.packages_dir()));
        let smapi_installer = Arc::new(match expected_smapi_sha256 {
            Some(hash) => {
                ProcessSmapiInstaller::new_with_expected_hash(paths.smapi_cache_dir(), hash)
            }
            None => ProcessSmapiInstaller::new(paths.smapi_cache_dir()),
        });
        // Synthetic lifecycle tests pin the SMAPI hash to keep the launcher from
        // observing unrelated processes on the host.
        let launcher = Arc::new(if expected_smapi_sha256.is_some() {
            DetachedGameLauncher::isolated()
        } else {
            DetachedGameLauncher::with_external_discovery(platform.process_discover_external)
        });
        let log_reader = Arc::new(SmapiSessionLogReader::new(None));
        let lock = Arc::new(FileInstanceLock::new(paths.lock_file_path()));
        let resources = Arc::new(manager_app::services::ResourceCoordinator::new());
        let discovery = platform.discovery.clone();
        let inspector = platform.inspector.clone();
        let runtime = platform.runtime.clone();
        let path_semantics = platform.path_semantics.clone();
        let downloader = Arc::new(ReqwestDownloader::new());
        let deployment = Arc::new(FilesystemDeploymentAdapter::new(paths.clone()));
        let staging = deployment.clone();
        let staging_verifier = Arc::new(StagedContentVerifier);
        let archive_inspector = Arc::new(SafeZipExtractor::new());

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
            path_semantics,
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
            resources.clone(),
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

        let freeze = Arc::new(manager_app::services::ProfileFreeze::new(
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
        ));

        let mods_service = Arc::new(
            manager_app::services::ModsService::new(
                packages_service.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                archive_inspector,
                staging.clone(),
                staging_verifier.clone(),
            )
            .with_freeze(freeze.clone()),
        );

        let operations_service = Arc::new(
            manager_app::services::OperationsService::new(
                resources.clone(),
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
            )
            .with_freeze(freeze.clone()),
        );

        let toggle_service = Arc::new(manager_app::services::ToggleService::new(
            resources.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            deployment.clone(),
            launcher.clone(),
            lock.clone(),
        ));

        let troubleshoot_service = Arc::new(manager_app::services::TroubleshootService::new(
            toggle_service.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
        ));

        let runtime_observer = Arc::new(manager_app::services::RuntimeObserver::new(
            repo.clone(),
            repo.clone(),
            repo.clone(),
            platform.inspector.clone(),
        ));

        let launch_service = Arc::new(
            manager_app::services::LaunchService::new(
                resources.clone(),
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
                runtime,
            )
            .with_runtime_observer(runtime_observer.clone())
            .with_known_good(Arc::new(manager_app::services::KnownGood::new(
                repo.clone(),
                repo.clone(),
                repo.clone(),
            ))),
        );

        let diagnostics_service = Arc::new(manager_app::services::DiagnosticsService::new(
            repo.clone(),
            log_reader.clone(),
            manager_app::services::HostEnvironment {
                operating_system: platform.operating_system,
                app_data_dir: paths.data_dir().to_path_buf(),
                cache_dir: paths.cache_dir().to_path_buf(),
                steam_roots: platform.discovery.describe_searched_locations(),
            },
        ));

        let health_service = Arc::new(
            manager_app::services::HealthService::new(
                repo.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
            )
            .with_runtime_observer(runtime_observer.clone()),
        );

        let bundle_service = Arc::new(
            manager_app::services::BundleService::new(
                repo.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                packages_service.clone(),
                profiles_service.clone(),
                mods_service.clone(),
                operations_service.clone(),
                toggle_service.clone(),
                Arc::new(manager_infra::ZipBundleArchive),
                paths.cache_dir().join("bundle-import"),
            )
            .with_settings(Arc::new(
                manager_infra::deployed_files::FilesystemDeployedFiles::new(paths.clone()),
            )),
        );

        let storage_service = Arc::new(
            manager_app::services::StorageCleanupService::new(
                resources.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                Arc::new(manager_infra::FilesystemStorageInventory::new(
                    paths.data_dir().to_path_buf(),
                    paths.packages_dir(),
                    paths.smapi_cache_dir(),
                )),
                lock.clone(),
            )
            .with_recovery_references(repo.clone()),
        );

        let profile_deletion_service =
            Arc::new(manager_app::services::ProfileDeletionService::new(
                resources.clone(),
                repo.clone(),
                repo.clone(),
                repo.clone(),
                Arc::new(
                    manager_infra::profile_folders::FilesystemProfileFolders::new(paths.clone()),
                ),
                launcher.clone(),
                lock.clone(),
            ));

        let saves_service = Arc::new(manager_app::services::SavesService::new(
            Arc::new(manager_infra::saves::FilesystemSaves::new(
                manager_infra::saves::default_saves_dir(),
                paths.data_dir().join("save-backups"),
            )),
            repo.clone(),
            repo.clone(),
            launcher.clone(),
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
            toggle: toggle_service,
            profile_deletion: profile_deletion_service,
            saves: saves_service,
            storage: storage_service,
            bundle: bundle_service,
            troubleshoot: troubleshoot_service,
        };

        let mods_queries =
            Arc::new(ModsQueries::new(repo.clone(), repo.clone()).with_history(repo.clone()));

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
            platform,
        })
    }
}
