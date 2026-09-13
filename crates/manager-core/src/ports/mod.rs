use crate::domain::{
    GameInstallation, InstalledMod, LaunchSession, Operation, OperationState, Package, Setup,
    SmapiInstallationRecord, WindowGeometry,
};
use crate::launch::{LaunchSpec, SessionVerificationBaseline, SessionVerificationResult};
use std::path::{Path, PathBuf};

pub trait StateRepository: Send + Sync {
    fn save_game(&self, game: &GameInstallation) -> Result<(), String>;
    fn get_game(&self, id: &str) -> Result<Option<GameInstallation>, String>;
    fn list_games(&self) -> Result<Vec<GameInstallation>, String>;

    fn save_setup(&self, setup: &Setup) -> Result<(), String>;
    fn get_setup(&self, id: &str) -> Result<Option<Setup>, String>;
    fn get_default_setup(&self, game_id: &str) -> Result<Option<Setup>, String>;

    fn save_package(&self, pkg: &Package) -> Result<(), String>;
    fn get_package(&self, hash: &str) -> Result<Option<Package>, String>;

    fn save_installed_mod(&self, mod_item: &InstalledMod) -> Result<(), String>;
    fn get_installed_mod(&self, id: &str) -> Result<Option<InstalledMod>, String>;
    fn list_installed_mods(&self, setup_id: &str) -> Result<Vec<InstalledMod>, String>;
    fn delete_installed_mod(&self, id: &str) -> Result<(), String>;

    fn save_operation(&self, op: &Operation) -> Result<(), String>;
    fn get_operation(&self, id: &str) -> Result<Option<Operation>, String>;
    fn update_operation_state(
        &self,
        id: &str,
        state: OperationState,
        error_json: Option<String>,
    ) -> Result<(), String>;
    fn list_unresolved_operations(&self) -> Result<Vec<Operation>, String>;

    fn save_launch_session(&self, session: &LaunchSession) -> Result<(), String>;
    fn get_launch_session(&self, id: &str) -> Result<Option<LaunchSession>, String>;
    fn get_latest_launch_session(
        &self,
        game_id: Option<&str>,
    ) -> Result<Option<LaunchSession>, String>;
    fn update_launch_session(&self, session: &LaunchSession) -> Result<(), String>;

    fn save_smapi_installation(&self, record: &SmapiInstallationRecord) -> Result<(), String>;
    fn get_smapi_installation(
        &self,
        game_id: &str,
    ) -> Result<Option<SmapiInstallationRecord>, String>;

    fn save_window_geometry(&self, geom: &WindowGeometry) -> Result<(), String>;
    fn get_window_geometry(&self) -> Result<Option<WindowGeometry>, String>;

    // Atomic combined operation commits
    fn commit_install_transaction(
        &self,
        operation_id: &str,
        installed_mod: &InstalledMod,
    ) -> Result<(), String> {
        self.commit_bundle_install_transaction(operation_id, std::slice::from_ref(installed_mod))
    }

    fn commit_bundle_install_transaction(
        &self,
        operation_id: &str,
        installed_mods: &[InstalledMod],
    ) -> Result<(), String>;

    fn commit_remove_transaction(
        &self,
        operation_id: &str,
        installed_mod_id: &str,
    ) -> Result<(), String> {
        self.commit_bundle_remove_transaction(operation_id, &[installed_mod_id.to_string()])
    }

    fn commit_bundle_remove_transaction(
        &self,
        operation_id: &str,
        installed_mod_ids: &[String],
    ) -> Result<(), String>;
}

pub trait PackageStore: Send + Sync {
    fn store_package(&self, source_zip: &Path) -> Result<Package, String>;
    fn get_package_path(&self, hash: &str) -> Result<PathBuf, String>;
}

pub trait SmapiInstaller: Send + Sync {
    fn install_smapi(
        &self,
        game_path: &Path,
        installer_archive: Option<&Path>,
    ) -> Result<SmapiInstallationRecord, String>;
}

pub trait GameLauncher: Send + Sync {
    fn launch_game(&self, spec: &LaunchSpec) -> Result<u32, String>;
    fn is_game_running(&self, pid: Option<u32>) -> bool;
    fn terminate_game(&self, pid: Option<u32>) -> Result<(), String>;
}

pub trait SessionLogReader: Send + Sync {
    fn capture_baseline(&self) -> Result<SessionVerificationBaseline, String>;
    fn verify_session(
        &self,
        baseline: &SessionVerificationBaseline,
        expected_mod_ids: &[String],
        expected_mods: &[InstalledMod],
    ) -> Result<SessionVerificationResult, String>;
    fn read_log_content(&self) -> Result<String, String>;
    fn log_file_path(&self) -> PathBuf;
}

pub trait InstanceLock: Send + Sync {
    fn acquire_guard(&self) -> Result<Box<dyn std::any::Any + Send + Sync>, String>;
}
