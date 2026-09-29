pub mod archive;
pub mod db;
pub mod deployment;
pub mod discovery;
pub mod http;
pub mod launcher;
pub mod lock;
pub mod log_reader;
pub mod package_store;
pub mod paths;
pub mod platform;
pub mod smapi_adapter;

pub use archive::SafeZipExtractor;
pub use db::SqliteStateRepository;
pub use deployment::FilesystemDeploymentAdapter;
pub use discovery::{PlatformGameInspector, SteamGameDiscovery};
#[cfg(unix)]
pub use discovery::{PosixGameInspector, PosixSteamLocator};
#[cfg(target_os = "windows")]
pub use discovery::{WindowsGameInspector, WindowsSteamLocator};
pub use http::ReqwestDownloader;
pub use launcher::DetachedGameLauncher;
pub use lock::FileInstanceLock;
pub use log_reader::{HostSmapiLogLocator, SmapiLogLocator, SmapiSessionLogReader};
pub use package_store::FilesystemPackageStore;
pub use paths::AppPaths;
pub use platform::host_runtime::{HostGameRuntime, TestGameRuntime};
pub use platform::HostPlatform;
pub use smapi_adapter::ProcessSmapiInstaller;
