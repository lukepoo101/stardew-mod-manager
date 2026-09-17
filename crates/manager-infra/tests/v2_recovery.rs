//! Crash-boundary recovery for plan-schema-v2 operations.
//!
//! Each test constructs the persisted steps plus the filesystem and database
//! evidence that a specific crash would leave behind, runs recovery, and then
//! asserts all three of operation state, filesystem evidence and database
//! ownership. Where recovery refuses to guess, the error contract is asserted
//! too: code, category, recoverability and operation id.

use manager_app::error::AppErrorCategory;
use manager_app::ports::deployment::{DeploymentPort, StagingPort};
use manager_app::ports::repositories::{
    AtomicMutationStore, DeploymentRepository, GameInstallationRepository, OperationRepository,
    PackageCatalogRepository, ProfileRepository, SmapiRepository,
};
use manager_app::services::OperationsService;
use manager_core::dependency::DependencyReport;
use manager_core::deployment::{
    DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{
    ArtifactHash, DeploymentId, GameInstallationId, OperationId, PackageComponentId,
    ProfileComponentId, ProfileId,
};
use manager_core::install::{ComponentManifest, InstallPlan, InventoryEntry, InventoryEntryType};
use manager_core::manifest::{Manifest, ModDependency};
use manager_core::operation::{
    Operation, OperationKind, OperationState, OperationStep, OperationStepKind, OperationStepState,
    OPERATION_PLAN_SCHEMA_V2,
};
use manager_core::package::{Acquisition, AcquisitionSource, PackageArtifact, PackageComponent};
use manager_core::profile::Profile;
use manager_infra::archive::StagedContentVerifier;
use manager_infra::db::SqliteStateRepository;
use manager_infra::deployment::FilesystemDeploymentAdapter;
use manager_infra::launcher::DetachedGameLauncher;
use manager_infra::lock::FileInstanceLock;
use manager_infra::paths::AppPaths;
use manager_infra::smapi_adapter::ProcessSmapiInstaller;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A filesystem deployment adapter whose evidence queries can be made to fail,
/// so "could not read evidence" can be told apart from "evidence absent".
#[derive(Default)]
struct Faults {
    fail_live_evidence: AtomicBool,
    fail_recovery_evidence: AtomicBool,
    fail_cleanup: AtomicBool,
    fail_create_staging: AtomicBool,
}

#[derive(Clone)]
struct ControllableDeployment {
    inner: FilesystemDeploymentAdapter,
    faults: Arc<Faults>,
}

impl StagingPort for ControllableDeployment {
    fn create_staging_dir(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
    ) -> manager_app::error::AppResult<PathBuf> {
        if self.faults.fail_create_staging.load(Ordering::SeqCst) {
            return Err(manager_app::error::AppError::filesystem(
                "Could not prepare the staging directory",
                "injected staging failure",
            ));
        }
        self.inner.create_staging_dir(profile_id, operation_id)
    }
    fn clean_staging_dir(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
    ) -> manager_app::error::AppResult<()> {
        if self.faults.fail_cleanup.load(Ordering::SeqCst) {
            return Err(manager_app::error::AppError::filesystem(
                "Could not clean the staging directory",
                "injected cleanup failure",
            ));
        }
        self.inner.clean_staging_dir(profile_id, operation_id)
    }
    fn staged_content_exists(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        relative_path: &str,
    ) -> manager_app::error::AppResult<bool> {
        self.inner
            .staged_content_exists(profile_id, operation_id, relative_path)
    }
}

impl DeploymentPort for ControllableDeployment {
    fn publish_deployment(
        &self,
        profile_id: &ProfileId,
        staged_folder: &std::path::Path,
        destination_rel_path: &str,
    ) -> manager_app::error::AppResult<PathBuf> {
        self.inner
            .publish_deployment(profile_id, staged_folder, destination_rel_path)
    }
    fn quarantine_deployment(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> manager_app::error::AppResult<PathBuf> {
        self.inner
            .quarantine_deployment(profile_id, operation_id, deployment_rel_path)
    }
    fn restore_quarantined_deployment(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> manager_app::error::AppResult<()> {
        self.inner
            .restore_quarantined_deployment(profile_id, operation_id, deployment_rel_path)
    }
    fn disable_deployment(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> manager_app::error::AppResult<()> {
        self.inner
            .disable_deployment(profile_id, deployment_rel_path)
    }
    fn enable_deployment(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> manager_app::error::AppResult<()> {
        self.inner
            .enable_deployment(profile_id, deployment_rel_path)
    }
    fn deployment_exists(
        &self,
        profile_id: &ProfileId,
        deployment_rel_path: &str,
    ) -> manager_app::error::AppResult<bool> {
        if self.faults.fail_live_evidence.load(Ordering::SeqCst) {
            return Err(manager_app::error::AppError::filesystem(
                "Could not read live deployment evidence",
                "injected evidence failure",
            ));
        }
        self.inner
            .deployment_exists(profile_id, deployment_rel_path)
    }
    fn recovery_deployment_exists(
        &self,
        profile_id: &ProfileId,
        operation_id: &OperationId,
        deployment_rel_path: &str,
    ) -> manager_app::error::AppResult<bool> {
        if self.faults.fail_recovery_evidence.load(Ordering::SeqCst) {
            return Err(manager_app::error::AppError::filesystem(
                "Could not read recovery evidence",
                "injected evidence failure",
            ));
        }
        self.inner
            .recovery_deployment_exists(profile_id, operation_id, deployment_rel_path)
    }
    fn get_profile_mods_root(&self, profile_id: &ProfileId) -> PathBuf {
        self.inner.get_profile_mods_root(profile_id)
    }
}

struct Harness {
    repo: Arc<SqliteStateRepository>,
    service: OperationsService,
    deployment: Arc<ControllableDeployment>,
    resources: Arc<manager_app::services::ResourceCoordinator>,
    faults: Arc<Faults>,
    game_id: GameInstallationId,
    game_root: PathBuf,
    profile: Profile,
    paths: AppPaths,
    _tmp: tempfile::TempDir,
}

fn harness() -> Harness {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("state.sqlite3")).unwrap());

    let game_root = tmp.path().join("Game");
    std::fs::create_dir_all(&game_root).unwrap();
    let game = GameInstallation {
        id: GameInstallationId::new(),
        canonical_root: game_root.clone(),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: chrono::Utc::now(),
    };
    repo.save_game(&game).unwrap();

    let profile = Profile::new(game.id, "Default");
    repo.save_profile(&profile).unwrap();

    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let faults = Arc::new(Faults::default());
    let deployment = Arc::new(ControllableDeployment {
        inner: FilesystemDeploymentAdapter::new(paths.clone()),
        faults: faults.clone(),
    });
    let smapi = Arc::new(ProcessSmapiInstaller::new(paths.smapi_cache_dir()));
    let resources = Arc::new(manager_app::services::ResourceCoordinator::new());

    let service = OperationsService::new(
        resources.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        repo.clone(),
        deployment.clone(),
        deployment.clone(),
        Arc::new(StagedContentVerifier),
        Arc::new(DetachedGameLauncher::isolated()),
        Arc::new(FileInstanceLock::new(paths.lock_file_path())),
        repo.clone(),
        smapi,
        repo.clone(),
    );

    Harness {
        repo,
        service,
        deployment,
        resources,
        faults,
        game_id: game.id,
        game_root,
        profile,
        paths,
        _tmp: tmp,
    }
}

impl Harness {
    fn launch_service(&self) -> manager_app::services::LaunchService {
        manager_app::services::LaunchService::new(
            self.resources.clone(),
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            Arc::new(DetachedGameLauncher::isolated()),
            self.deployment.clone(),
            Arc::new(manager_infra::log_reader::SmapiSessionLogReader::new(None)),
            Arc::new(FileInstanceLock::new(self.paths.lock_file_path())),
            Arc::new(manager_infra::TestGameRuntime::for_platform(
                manager_core::game::OperatingSystem::Linux,
            )),
        )
    }

    fn smapi_service(&self) -> manager_app::services::SmapiService {
        let installer = Arc::new(ProcessSmapiInstaller::new(self.paths.smapi_cache_dir()));
        manager_app::services::SmapiService::new(
            self.resources.clone(),
            self.repo.clone(),
            self.repo.clone(),
            installer.clone(),
            installer,
            Arc::new(manager_infra::http::ReqwestDownloader::new()),
            self.paths.smapi_cache_dir(),
            self.repo.clone(),
            Arc::new(DetachedGameLauncher::isolated()),
            Arc::new(FileInstanceLock::new(self.paths.lock_file_path())),
        )
    }

    fn mods_service(&self) -> manager_app::services::ModsService {
        let packages = Arc::new(manager_app::services::PackagesService::new(
            self.repo.clone(),
            Arc::new(manager_infra::package_store::FilesystemPackageStore::new(
                self.paths.packages_dir(),
            )),
        ));
        manager_app::services::ModsService::new(
            packages,
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            self.repo.clone(),
            Arc::new(manager_infra::archive::SafeZipExtractor::new()),
            self.deployment.clone(),
            Arc::new(StagedContentVerifier),
        )
    }

    /// Saves an artifact and acquisition row for an arbitrary content hash.
    fn save_artifact_for(&self, hash: &str) -> ArtifactHash {
        let hash = ArtifactHash::new(hash);
        self.repo
            .save_artifact(&PackageArtifact {
                hash: hash.clone(),
                byte_size: 7,
                storage_relative_path: format!("packages/{}.zip", hash.as_str()),
                first_seen_at: chrono::Utc::now(),
            })
            .unwrap();
        self.repo
            .save_acquisition(&Acquisition {
                id: manager_core::ids::AcquisitionId::new(),
                artifact_hash: hash.clone(),
                original_filename: "Example.zip".into(),
                expected_hash: Some(hash.clone()),
                source: AcquisitionSource::LocalFile,
                source_metadata: None,
                acquired_at: chrono::Utc::now(),
            })
            .unwrap();
        hash
    }

    /// Rewrites the publish step so it records the deployment identity, exactly
    /// as execution does before the commit.
    fn rewrite_publish_step(&self, operation_id: &OperationId, deployment_id: &DeploymentId) {
        self.repo
            .save_operation_step(&OperationStep {
                operation_id: *operation_id,
                step_index: 4,
                step_kind: OperationStepKind::PublishDeployment.as_str().to_string(),
                state: OperationStepState::Completed,
                payload_json: serde_json::json!({
                    "deployment_id": deployment_id.to_string(),
                })
                .to_string(),
                started_at: Some(chrono::Utc::now()),
                completed_at: Some(chrono::Utc::now()),
                error_json: None,
            })
            .unwrap();
    }

    /// The atomic install payload a real execution builds, with a caller-chosen
    /// revision so a rejected commit can be exercised.
    fn sample_install_commit(
        &self,
        operation: &Operation,
        deployment_id: DeploymentId,
        target: &str,
        expected_profile_revision: u64,
    ) -> manager_app::ports::repositories::InstallCommit {
        use manager_app::ports::repositories::{CommitStep, InstallCommit};

        let artifact_hash = self.save_artifact_for(&"a".repeat(64));
        let component_id = PackageComponentId::new();
        let component = PackageComponent {
            id: component_id,
            artifact_hash: artifact_hash.clone(),
            unique_id: "Tests.Example".into(),
            name: "Example".into(),
            author: "Tests".into(),
            version: "1.0.0".into(),
            description: None,
            relative_component_root: String::new(),
            raw_manifest: MANIFEST_RAW.into(),
            manifest: manifest(),
        };
        let profile_component = ProfileComponent {
            id: ProfileComponentId::new(),
            profile_id: self.profile.id,
            deployment_id,
            package_component_id: component_id,
            enabled: true,
            installed_reason: InstalledReason::Direct,
        };
        InstallCommit {
            operation_id: operation.id,
            profile_id: self.profile.id,
            expected_profile_revision,
            artifact: PackageArtifact {
                hash: artifact_hash.clone(),
                byte_size: 7,
                storage_relative_path: "packages/Example.zip".into(),
                first_seen_at: chrono::Utc::now(),
            },
            acquisition: Acquisition {
                id: manager_core::ids::AcquisitionId::new(),
                artifact_hash: artifact_hash.clone(),
                original_filename: "Example.zip".into(),
                expected_hash: Some(artifact_hash.clone()),
                source: AcquisitionSource::LocalFile,
                source_metadata: None,
                acquired_at: chrono::Utc::now(),
            },
            package_components: vec![component],
            deployment: ProfileDeployment {
                id: deployment_id,
                profile_id: self.profile.id,
                artifact_hash,
                root_relative_path: target.to_string(),
                installed_at: chrono::Utc::now(),
                state: DeploymentState::Present,
            },
            profile_components: vec![profile_component],
            effects: Vec::new(),
            commit_step: CommitStep {
                index: 5,
                kind: OperationStepKind::CommitInstallDatabase
                    .as_str()
                    .to_string(),
                payload_json: serde_json::json!({ "deployment_rel_path": target }).to_string(),
            },
        }
    }
}

fn write_mod_zip(path: &std::path::Path) {
    use std::io::Write;
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("Example/manifest.json", options).unwrap();
    zip.write_all(
        br#"{"Name":"Example","Author":"Tests","Version":"1.0.0","UniqueID":"Tests.Example","EntryDll":"Example.dll"}"#,
    )
    .unwrap();
    zip.start_file("Example/Example.dll", options).unwrap();
    zip.write_all(b"fixture").unwrap();
    zip.finish().unwrap();
}

const MANIFEST_RAW: &str = "{\"Name\":\"Example\"}";

fn manifest() -> Manifest {
    Manifest {
        unique_id: "Tests.Example".into(),
        name: "Example".into(),
        author: "Tests".into(),
        version: "1.0.0".into(),
        description: None,
        entry_dll: None,
        minimum_api_version: None,
        minimum_game_version: None,
        update_keys: Vec::new(),
        dependencies: Vec::<ModDependency>::new(),
        content_pack_for: None,
    }
}

fn dependency_report() -> DependencyReport {
    DependencyReport {
        is_installable: true,
        smapi_compatible: true,
        smapi_required_version: None,
        current_smapi_version: None,
        duplicate_id: false,
        findings: Vec::new(),
    }
}
impl Harness {
    fn mods_dir(&self) -> PathBuf {
        self.paths.profile_mods_dir(&self.profile.id)
    }

    fn recovery_dir(&self, operation_id: &OperationId, folder: &str) -> PathBuf {
        self.paths
            .profile_recovery_dir(&self.profile.id, operation_id)
            .join(folder)
    }

    fn staging_dir(&self, operation_id: &OperationId, folder: &str) -> PathBuf {
        self.paths
            .profile_staging_dir(&self.profile.id, operation_id)
            .join(folder)
    }

    /// Writes a mod folder into the profile's live Mods directory.
    fn publish_folder(&self, folder: &str) {
        let live = self.mods_dir().join(folder);
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("Example.dll"), b"fixture").unwrap();
    }

    fn quarantine_folder(&self, operation_id: &OperationId, folder: &str) {
        let target = self.recovery_dir(operation_id, folder);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("Example.dll"), b"fixture").unwrap();
    }

    /// Stages a mod folder and returns the trusted inventory for it.
    fn stage_folder(&self, operation_id: &OperationId, folder: &str) -> Vec<InventoryEntry> {
        let staged = self.staging_dir(operation_id, folder);
        std::fs::create_dir_all(&staged).unwrap();
        let body = b"fixture";
        std::fs::write(staged.join("Example.dll"), body).unwrap();
        vec![InventoryEntry {
            relative_path: "Example.dll".to_string(),
            entry_type: InventoryEntryType::File,
            size_bytes: body.len() as u64,
            sha256_hash: None,
        }]
    }

    fn save_artifact_and_acquisition(&self) -> String {
        let hash = ArtifactHash::new("a".repeat(64));
        self.repo
            .save_artifact(&PackageArtifact {
                hash: hash.clone(),
                byte_size: 7,
                storage_relative_path: "packages/Example.zip".into(),
                first_seen_at: chrono::Utc::now(),
            })
            .unwrap();
        self.repo
            .save_acquisition(&Acquisition {
                id: manager_core::ids::AcquisitionId::new(),
                artifact_hash: hash.clone(),
                original_filename: "Example.zip".into(),
                expected_hash: Some(hash.clone()),
                source: AcquisitionSource::LocalFile,
                source_metadata: None,
                acquired_at: chrono::Utc::now(),
            })
            .unwrap();
        hash.as_str().to_string()
    }

    fn install_plan_json(
        &self,
        hash: &str,
        folder: &str,
        inventory: Vec<InventoryEntry>,
    ) -> String {
        let plan = InstallPlan {
            plan_id: "plan".into(),
            setup_id: self.profile.id.to_string(),
            package_hash: hash.to_string(),
            original_filename: "Example.zip".into(),
            mod_folder_name: folder.to_string(),
            manifest: manifest(),
            raw_manifest: MANIFEST_RAW.to_string(),
            file_inventory: vec!["Example.dll".into()],
            trusted_inventory: inventory,
            dependency_report: dependency_report(),
            component_manifests: Vec::<ComponentManifest>::new(),
        };
        serde_json::to_string(&plan).unwrap()
    }

    fn removal_plan_json(
        &self,
        deployment_id: DeploymentId,
        folder: &str,
        component_ids: &[ProfileComponentId],
    ) -> String {
        serde_json::json!({
            "profile_id": self.profile.id.to_string(),
            "deployment_id": deployment_id.to_string(),
            "deployment_rel_path": folder,
            "removed_profile_component_ids": component_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// The deployment row the database would own after a completed install.
    fn own_deployment(&self, folder: &str) -> (DeploymentId, Vec<ProfileComponentId>) {
        // A deployment row has a foreign key onto the retained artifact.
        let hash = ArtifactHash::new(self.save_artifact_and_acquisition());
        let component_id = PackageComponentId::new();
        self.repo
            .save_package_component(&PackageComponent {
                id: component_id,
                artifact_hash: hash.clone(),
                unique_id: "Tests.Example".into(),
                name: "Example".into(),
                author: "Tests".into(),
                version: "1.0.0".into(),
                description: None,
                relative_component_root: String::new(),
                raw_manifest: MANIFEST_RAW.into(),
                manifest: manifest(),
            })
            .unwrap();
        let deployment_id = DeploymentId::new();
        self.repo
            .save_deployment(&ProfileDeployment {
                id: deployment_id,
                profile_id: self.profile.id,
                artifact_hash: hash,
                root_relative_path: folder.into(),
                installed_at: chrono::Utc::now(),
                state: DeploymentState::Present,
            })
            .unwrap();
        let component = ProfileComponent {
            id: ProfileComponentId::new(),
            profile_id: self.profile.id,
            deployment_id,
            package_component_id: component_id,
            enabled: true,
            installed_reason: InstalledReason::Direct,
        };
        self.repo.save_profile_component(&component).unwrap();
        (deployment_id, vec![component.id])
    }

    fn save_operation(
        &self,
        op: &Operation,
        steps: &[(u32, OperationStepKind, OperationStepState)],
    ) {
        self.repo.create_operation(op).unwrap();
        for (index, kind, state) in steps {
            self.repo
                .save_operation_step(&OperationStep {
                    operation_id: op.id,
                    step_index: *index,
                    step_kind: kind.as_str().to_string(),
                    state: *state,
                    payload_json: "{}".to_string(),
                    started_at: Some(chrono::Utc::now()),
                    completed_at: Some(chrono::Utc::now()),
                    error_json: None,
                })
                .unwrap();
        }
    }

    fn operation(&self, id: &OperationId) -> Operation {
        self.repo.get_operation(id).unwrap().unwrap()
    }

    fn step(&self, id: &OperationId, kind: OperationStepKind) -> Option<OperationStepState> {
        self.repo
            .list_operation_steps(id)
            .unwrap()
            .into_iter()
            .find(|step| OperationStepKind::parse(&step.step_kind) == Some(kind))
            .map(|step| step.state)
    }

    fn install_operation(
        &self,
        _folder: &str,
        plan_json: String,
        state: OperationState,
    ) -> Operation {
        Operation {
            id: OperationId::new(),
            kind: OperationKind::ModInstall,
            state,
            game_installation_id: Some(self.game_id),
            profile_id: Some(self.profile.id),
            expected_profile_revision: Some(self.profile.revision),
            plan_schema_version: OPERATION_PLAN_SCHEMA_V2,
            plan_json,
            progress_current: None,
            progress_total: None,
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
        }
    }

    fn removal_operation(&self, plan_json: String, state: OperationState) -> Operation {
        Operation {
            kind: OperationKind::ModRemove,
            ..self.install_operation("unused", plan_json, state)
        }
    }
}
// ---------------------------------------------------------------------------
// Install crash boundaries
// ---------------------------------------------------------------------------

#[test]
fn a_plan_that_was_prepared_before_publication_is_cancelled() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let operation = h.install_operation(
        "Example",
        h.install_plan_json(&hash, "Example", Vec::new()),
        OperationState::Prepared,
    );
    h.save_operation(&operation, &[]);

    h.service.retry_recovery().unwrap();

    assert_eq!(h.operation(&operation.id).state, OperationState::Cancelled);
    assert!(!h.mods_dir().join("Example").exists());
}

#[test]
fn a_publication_that_never_happened_is_republished_from_its_staged_source() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let operation = h.install_operation("Example", String::new(), OperationState::Committing);
    let inventory = h.stage_folder(&operation.id, "Example");
    let plan_json = h.install_plan_json(&hash, "Example", inventory);
    let operation = Operation {
        plan_json,
        ..operation
    };
    h.save_operation(
        &operation,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Running,
        )],
    );

    h.service.retry_recovery().unwrap();

    assert_eq!(h.operation(&operation.id).state, OperationState::Succeeded);
    assert!(h.mods_dir().join("Example").join("Example.dll").exists());
    assert_eq!(
        h.step(&operation.id, OperationStepKind::CommitInstallDatabase),
        Some(OperationStepState::Completed)
    );
    // The database now owns the deployment.
    let deployments = h.repo.list_deployments_for_profile(&h.profile.id).unwrap();
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].root_relative_path, "Example");
}

#[test]
fn a_live_folder_from_an_interrupted_commit_is_adopted_by_the_atomic_commit() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let operation = h.install_operation("Example", String::new(), OperationState::Committing);
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    let operation = Operation {
        plan_json,
        ..operation
    };
    h.publish_folder("Example");
    h.save_operation(
        &operation,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Running,
        )],
    );

    h.service.retry_recovery().unwrap();

    assert_eq!(h.operation(&operation.id).state, OperationState::Succeeded);
    assert!(h.mods_dir().join("Example").join("Example.dll").exists());
    assert_eq!(
        h.step(&operation.id, OperationStepKind::PublishDeployment),
        Some(OperationStepState::Completed)
    );
    assert_eq!(
        h.repo
            .list_deployments_for_profile(&h.profile.id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn an_install_whose_atomic_commit_already_happened_is_completed() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let (_, _) = h.own_deployment("Example");
    h.publish_folder("Example");
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    let operation = h.install_operation("Example", plan_json, OperationState::Committing);
    h.save_operation(
        &operation,
        &[
            (
                4,
                OperationStepKind::PublishDeployment,
                OperationStepState::Completed,
            ),
            (
                5,
                OperationStepKind::CommitInstallDatabase,
                OperationStepState::Running,
            ),
        ],
    );

    h.service.retry_recovery().unwrap();

    assert_eq!(h.operation(&operation.id).state, OperationState::Succeeded);
    assert!(h.mods_dir().join("Example").join("Example.dll").exists());
}

#[test]
fn both_a_live_and_a_quarantined_copy_require_manual_reconciliation() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    let operation = h.install_operation("Example", plan_json, OperationState::Committing);
    h.publish_folder("Example");
    h.quarantine_folder(&operation.id, "Example");
    h.save_operation(
        &operation,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Running,
        )],
    );

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "INSTALL_EVIDENCE_AMBIGUOUS");
    assert_eq!(error.category, AppErrorCategory::Recovery);
    assert_eq!(
        error.operation_id.as_deref(),
        Some(operation.id.to_string().as_str())
    );
    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::RecoveryRequired);
    assert_eq!(
        persisted.error_code.as_deref(),
        Some("INSTALL_EVIDENCE_AMBIGUOUS")
    );
    assert!(h.mods_dir().join("Example").exists());
    assert!(h.recovery_dir(&operation.id, "Example").exists());
}

#[test]
fn an_unreadable_evidence_query_never_becomes_absent_evidence() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    let operation = h.install_operation("Example", plan_json, OperationState::Committing);
    h.publish_folder("Example");
    h.save_operation(
        &operation,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Running,
        )],
    );
    h.faults.fail_live_evidence.store(true, Ordering::SeqCst);

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "RECOVERY_EVIDENCE_UNREADABLE");
    assert_eq!(error.category, AppErrorCategory::Recovery);
    assert_eq!(
        error.recoverability,
        manager_app::error::Recoverability::RequiresManualIntervention
    );
    assert_eq!(
        error.operation_id.as_deref(),
        Some(operation.id.to_string().as_str())
    );
    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::RecoveryRequired);
    // Nothing was moved: the folder is exactly where the crash left it.
    assert!(h.mods_dir().join("Example").exists());
}

#[test]
fn a_stale_plan_publication_is_compensated_into_a_safe_unsuccessful_end() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    // The plan was validated against the previous revision.
    let operation = Operation {
        expected_profile_revision: Some(h.profile.revision + 1),
        ..h.install_operation("Example", plan_json, OperationState::Committing)
    };
    h.publish_folder("Example");
    h.save_operation(
        &operation,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Completed,
        )],
    );

    h.service.retry_recovery().unwrap();

    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::Failed);
    assert_eq!(
        persisted.error_code.as_deref(),
        Some("INSTALL_COMPENSATED_STALE_PLAN")
    );
    assert!(!h.mods_dir().join("Example").exists());
    assert!(h
        .recovery_dir(&operation.id, "Example")
        .join("Example.dll")
        .exists());
    assert!(h
        .repo
        .list_deployments_for_profile(&h.profile.id)
        .unwrap()
        .is_empty());
}

#[test]
fn a_failed_compensation_requires_manual_reconciliation() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    let operation = Operation {
        expected_profile_revision: Some(h.profile.revision + 1),
        ..h.install_operation("Example", plan_json, OperationState::Committing)
    };
    h.publish_folder("Example");
    // Block the recovery tree so the published folder cannot be moved out.
    let recovery_root = h.paths.profile_recovery_dir(&h.profile.id, &operation.id);
    std::fs::create_dir_all(recovery_root.parent().unwrap()).unwrap();
    std::fs::write(&recovery_root, b"blocks quarantine").unwrap();
    h.save_operation(
        &operation,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Completed,
        )],
    );

    let error = h.service.retry_recovery().unwrap_err();

    // The diagnosis is the underlying filesystem failure; the recovery contract
    // is what the caller has to act on.
    assert_eq!(error.category, AppErrorCategory::Recovery);
    assert_eq!(
        error.recoverability,
        manager_app::error::Recoverability::RequiresManualIntervention
    );
    assert_eq!(
        error.operation_id.as_deref(),
        Some(operation.id.to_string().as_str())
    );
    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::RecoveryRequired);
    assert_eq!(
        persisted.error_code.as_deref(),
        Some("INSTALL_COMPENSATION_FAILED")
    );
    assert!(h.mods_dir().join("Example").exists());
}

#[test]
fn an_install_that_cannot_rebuild_its_commit_payload_stays_in_recovery() {
    let h = harness();
    // No artifact row: the atomic commit payload cannot be rebuilt.
    let plan_json = h.install_plan_json(&"b".repeat(64), "Example", Vec::new());
    let operation = h.install_operation("Example", plan_json, OperationState::Committing);
    h.publish_folder("Example");
    h.save_operation(
        &operation,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Completed,
        )],
    );

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "INSTALL_COMMIT_PAYLOAD_UNAVAILABLE");
    assert_eq!(
        h.operation(&operation.id).state,
        OperationState::RecoveryRequired
    );
    assert!(h.mods_dir().join("Example").exists());
}

#[test]
fn an_install_that_never_published_requires_a_fresh_plan() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    let operation = h.install_operation("Example", plan_json, OperationState::Committing);
    h.save_operation(
        &operation,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Running,
        )],
    );

    h.service.retry_recovery().unwrap();

    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::Failed);
    assert_eq!(
        persisted.error_code.as_deref(),
        Some("INSTALL_NOT_PUBLISHED")
    );
    assert!(!h.mods_dir().join("Example").exists());
}
// ---------------------------------------------------------------------------
// Removal crash boundaries
// ---------------------------------------------------------------------------

fn removal_case(
    state: OperationState,
    steps: &[(u32, OperationStepKind, OperationStepState)],
) -> (Harness, Operation, String) {
    let h = harness();
    let folder = "Example";
    let (deployment_id, component_ids) = h.own_deployment(folder);
    let plan_json = h.removal_plan_json(deployment_id, folder, &component_ids);
    let operation = h.removal_operation(plan_json, state);
    h.save_operation(&operation, steps);
    (h, operation, folder.to_string())
}

#[test]
fn a_removal_before_quarantine_quarantines_and_commits() {
    let (h, operation, folder) = removal_case(
        OperationState::Committing,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Running,
        )],
    );
    h.publish_folder(&folder);

    h.service.retry_recovery().unwrap();

    assert_eq!(h.operation(&operation.id).state, OperationState::Succeeded);
    assert!(!h.mods_dir().join(&folder).exists());
    assert!(h.recovery_dir(&operation.id, &folder).exists());
    assert!(h
        .repo
        .list_profile_components(&h.profile.id)
        .unwrap()
        .is_empty());
    // The database no longer owns an active deployment for that path.
    let deployments = h.repo.list_deployments_for_profile(&h.profile.id).unwrap();
    assert!(deployments
        .iter()
        .all(|deployment| deployment.state == DeploymentState::Quarantined));
}

#[test]
fn a_removal_whose_quarantine_completed_still_commits() {
    let (h, operation, folder) = removal_case(
        OperationState::Committing,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Completed,
        )],
    );
    h.quarantine_folder(&operation.id, &folder);

    h.service.retry_recovery().unwrap();

    assert_eq!(h.operation(&operation.id).state, OperationState::Succeeded);
    assert!(!h.mods_dir().join(&folder).exists());
    assert!(h.recovery_dir(&operation.id, &folder).exists());
    assert_eq!(
        h.step(&operation.id, OperationStepKind::CommitRemovalDatabase),
        Some(OperationStepState::Completed)
    );
}

#[test]
fn a_removal_that_already_committed_is_completed() {
    let (h, operation, folder) = removal_case(
        OperationState::Committing,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Completed,
        )],
    );
    h.quarantine_folder(&operation.id, &folder);
    // The database no longer owns the deployment: the components are gone and
    // the deployment row is marked quarantined.
    for component in h.repo.list_profile_components(&h.profile.id).unwrap() {
        h.repo.delete_profile_component(&component.id).unwrap();
    }
    for mut deployment in h.repo.list_deployments_for_profile(&h.profile.id).unwrap() {
        deployment.state = DeploymentState::Quarantined;
        h.repo.save_deployment(&deployment).unwrap();
    }

    h.service.retry_recovery().unwrap();

    assert_eq!(h.operation(&operation.id).state, OperationState::Succeeded);
    assert_eq!(
        h.operation(&operation.id).error_code.as_deref(),
        Some("RECOVERED_AFTER_REMOVAL_COMMIT")
    );
    assert!(!h.mods_dir().join(&folder).exists());
}

#[test]
fn a_removal_failure_that_restores_its_deployment_ends_unsuccessfully() {
    let (h, operation, folder) = removal_case(
        OperationState::Committing,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Completed,
        )],
    );
    h.quarantine_folder(&operation.id, &folder);
    // A revision the database has moved past makes the atomic commit fail.
    let mut profile = h.profile.clone();
    profile.bump_revision();
    h.repo.save_profile(&profile).unwrap();

    h.service.retry_recovery().unwrap();

    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::Failed);
    assert_eq!(persisted.error_code.as_deref(), Some("COMMIT_FAILED"));
    // The deployment was restored and the database still owns it.
    assert!(h.mods_dir().join(&folder).join("Example.dll").exists());
    assert!(!h.recovery_dir(&operation.id, &folder).exists());
    assert_eq!(
        h.repo.list_profile_components(&h.profile.id).unwrap().len(),
        1
    );
}

#[test]
fn a_removal_failure_that_cannot_restore_requires_manual_reconciliation() {
    let (h, operation, folder) = removal_case(
        OperationState::Committing,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Completed,
        )],
    );
    h.quarantine_folder(&operation.id, &folder);
    let mut profile = h.profile.clone();
    profile.bump_revision();
    h.repo.save_profile(&profile).unwrap();
    // Block the restore target so compensation cannot succeed: the profile's
    // Mods path is occupied by a file, not a directory.
    std::fs::create_dir_all(h.mods_dir().parent().unwrap()).unwrap();
    std::fs::write(h.mods_dir(), b"not a directory").unwrap();

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.category, AppErrorCategory::Recovery);
    assert_eq!(
        error.recoverability,
        manager_app::error::Recoverability::RequiresManualIntervention
    );
    assert_eq!(
        error.operation_id.as_deref(),
        Some(operation.id.to_string().as_str())
    );
    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::RecoveryRequired);
    assert_eq!(persisted.error_code.as_deref(), Some("COMMIT_FAILED"));
    // Every piece of evidence is retained.
    assert!(h.recovery_dir(&operation.id, &folder).exists());
}

#[test]
fn a_removal_with_both_copies_present_requires_manual_reconciliation() {
    let (h, operation, folder) = removal_case(
        OperationState::Committing,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Running,
        )],
    );
    h.publish_folder(&folder);
    h.quarantine_folder(&operation.id, &folder);

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "REMOVAL_EVIDENCE_AMBIGUOUS");
    assert_eq!(error.category, AppErrorCategory::Recovery);
    assert_eq!(
        h.operation(&operation.id).state,
        OperationState::RecoveryRequired
    );
    assert!(h.mods_dir().join(&folder).exists());
    assert!(h.recovery_dir(&operation.id, &folder).exists());
}

#[test]
fn a_removal_with_no_evidence_while_the_database_owns_it_requires_manual_reconciliation() {
    let (h, operation, _folder) = removal_case(
        OperationState::Committing,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Running,
        )],
    );

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "REMOVAL_EVIDENCE_MISSING");
    assert_eq!(error.category, AppErrorCategory::Recovery);
    assert_eq!(
        h.operation(&operation.id).state,
        OperationState::RecoveryRequired
    );
    assert_eq!(
        h.repo.list_profile_components(&h.profile.id).unwrap().len(),
        1,
        "the database still owns the deployment"
    );
}

// ---------------------------------------------------------------------------
// SMAPI crash boundaries
// ---------------------------------------------------------------------------

fn smapi_operation(
    h: &Harness,
    steps: &[(u32, OperationStepKind, OperationStepState)],
) -> Operation {
    let operation = Operation {
        id: OperationId::new(),
        kind: OperationKind::SmapiSetup,
        state: OperationState::Committing,
        game_installation_id: Some(h.game_id),
        profile_id: None,
        expected_profile_revision: None,
        plan_schema_version: OPERATION_PLAN_SCHEMA_V2,
        plan_json: serde_json::json!({ "release_policy_id": "pinned" }).to_string(),
        progress_current: None,
        progress_total: None,
        error_code: None,
        error_json: None,
        cancellation_requested: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        completed_at: None,
    };
    h.save_operation(&operation, steps);
    operation
}

fn write_smapi_files(h: &Harness, version: Option<&str>) {
    let game_root = h.game_root.clone();
    std::fs::write(game_root.join("StardewModdingAPI.dll"), b"dll").unwrap();
    std::fs::create_dir_all(game_root.join("smapi-internal")).unwrap();
    if let Some(version) = version {
        std::fs::write(
            game_root.join("StardewModdingAPI.deps.json"),
            format!(
                "{{\"targets\":{{\"net6.0/linux-x64\":{{\"StardewModdingAPI/{version}\":{{}}}}}}}}"
            ),
        )
        .unwrap();
    }
}

#[test]
fn an_interrupted_smapi_install_without_files_is_a_terminal_failure() {
    let h = harness();
    let operation = smapi_operation(
        &h,
        &[(
            2,
            OperationStepKind::InstallSmapiFiles,
            OperationStepState::Running,
        )],
    );

    h.service.retry_recovery().unwrap();

    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::Failed);
    assert_eq!(persisted.error_code.as_deref(), Some("SMAPI_NOT_PRESENT"));
    assert_eq!(
        h.step(&operation.id, OperationStepKind::InstallSmapiFiles),
        Some(OperationStepState::Failed)
    );
}

#[test]
fn smapi_files_without_managed_state_are_reconciled_into_success() {
    let h = harness();
    write_smapi_files(&h, Some("4.1.10"));
    let operation = smapi_operation(
        &h,
        &[(
            2,
            OperationStepKind::InstallSmapiFiles,
            OperationStepState::Running,
        )],
    );

    h.service.retry_recovery().unwrap();

    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::Succeeded);
    assert_eq!(
        persisted.error_code.as_deref(),
        Some("RECOVERED_SMAPI_STATE")
    );
    assert_eq!(
        h.step(&operation.id, OperationStepKind::PersistSmapiState),
        Some(OperationStepState::Completed)
    );
    assert!(h.repo.get_smapi_installation(&h.game_id).unwrap().is_some());
}

#[test]
fn smapi_files_without_an_establishable_version_stay_unrecovered() {
    let h = harness();
    write_smapi_files(&h, None);
    let operation = smapi_operation(
        &h,
        &[(
            2,
            OperationStepKind::InstallSmapiFiles,
            OperationStepState::Running,
        )],
    );

    h.service.retry_recovery().unwrap();

    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::Failed);
    assert_eq!(
        persisted.error_code.as_deref(),
        Some("SMAPI_RECOVERY_VERSION_UNKNOWN")
    );
    assert!(h.repo.get_smapi_installation(&h.game_id).unwrap().is_none());
}
#[test]
fn startup_recovery_continues_after_one_operation_remains_unresolved() {
    let h = harness();

    // One ambiguous operation that has to stay unresolved.
    let hash = h.save_artifact_and_acquisition();
    let plan_json = h.install_plan_json(&hash, "Ambiguous", Vec::new());
    let ambiguous = h.install_operation("Ambiguous", plan_json, OperationState::Committing);
    h.publish_folder("Ambiguous");
    h.quarantine_folder(&ambiguous.id, "Ambiguous");
    h.save_operation(
        &ambiguous,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Running,
        )],
    );

    // One independent operation that can be reconciled.
    let (deployment_id, component_ids) = h.own_deployment("Recoverable");
    let plan_json = h.removal_plan_json(deployment_id, "Recoverable", &component_ids);
    let recoverable = h.removal_operation(plan_json, OperationState::Committing);
    h.save_operation(
        &recoverable,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Completed,
        )],
    );
    h.quarantine_folder(&recoverable.id, "Recoverable");

    h.service
        .recover_on_startup()
        .expect("an ambiguous operation must not stop startup recovery");

    assert_eq!(
        h.operation(&ambiguous.id).state,
        OperationState::RecoveryRequired
    );
    assert_eq!(
        h.operation(&recoverable.id).state,
        OperationState::Succeeded
    );
}
// ---------------------------------------------------------------------------
// Durable resource scoping
// ---------------------------------------------------------------------------

#[test]
fn durable_resource_conflicts_are_scoped_to_the_claimed_resource_and_survive_a_restart() {
    use manager_app::services::resources::{ensure_resources_available, ResourceClaim};
    use manager_core::operation::{AccessMode, OperationResource, ResourceKind};

    let h = harness();
    let other = Profile::new(h.game_id, "Other");
    h.repo.save_profile(&other).unwrap();

    // A historical recovery-required operation owns a profile write. Nothing
    // in-process remembers it: this is exactly the state left after a restart.
    let hash = h.save_artifact_and_acquisition();
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    let operation = h.install_operation("Example", plan_json, OperationState::RecoveryRequired);
    h.save_operation(&operation, &[]);
    h.repo
        .save_operation_resource(&OperationResource {
            operation_id: operation.id,
            resource_kind: ResourceKind::Profile,
            resource_id: h.profile.id.to_string(),
            access_mode: AccessMode::Write,
        })
        .unwrap();

    let blocked = ensure_resources_available(
        &*h.repo,
        &[ResourceClaim::write(
            ResourceKind::Profile,
            h.profile.id.to_string(),
        )],
        None,
    )
    .expect_err("an unresolved write must block a new write on the same profile");
    assert_eq!(blocked.code, "PROFILE_OPERATION_UNRESOLVED");
    assert_eq!(
        blocked.recoverability,
        manager_app::error::Recoverability::RequiresManualIntervention
    );

    // A different profile is independent.
    ensure_resources_available(
        &*h.repo,
        &[ResourceClaim::write(
            ResourceKind::Profile,
            other.id.to_string(),
        )],
        None,
    )
    .expect("an unresolved operation on one profile must not block another");

    // A read of the same profile also conflicts with the write claim: that is
    // what stops a launch from racing a recovery.
    let read_blocked = ensure_resources_available(
        &*h.repo,
        &[ResourceClaim::read(
            ResourceKind::Profile,
            h.profile.id.to_string(),
        )],
        None,
    )
    .expect_err("a read of a write-claimed profile must conflict");
    assert_eq!(read_blocked.code, "PROFILE_OPERATION_UNRESOLVED");

    // A read of a different profile, and any claim on another resource kind,
    // are independent.
    ensure_resources_available(
        &*h.repo,
        &[ResourceClaim::read(
            ResourceKind::Profile,
            other.id.to_string(),
        )],
        None,
    )
    .expect("a different profile is independent");
    ensure_resources_available(
        &*h.repo,
        &[ResourceClaim::write(
            ResourceKind::GameInstallation,
            h.game_id.to_string(),
        )],
        None,
    )
    .expect("a different resource kind is independent");
}
// ---------------------------------------------------------------------------
// Conjunctive recovery proofs
// ---------------------------------------------------------------------------

/// A database row is only half the proof: the folder it describes must be live.
#[test]
fn an_owned_install_with_a_missing_folder_is_not_a_success() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let (deployment_id, _) = h.own_deployment("Example");
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    let operation = h.install_operation("Example", plan_json, OperationState::Committing);
    h.save_operation(
        &operation,
        &[
            (
                4,
                OperationStepKind::PublishDeployment,
                OperationStepState::Completed,
            ),
            (
                5,
                OperationStepKind::CommitInstallDatabase,
                OperationStepState::Completed,
            ),
        ],
    );
    h.rewrite_publish_step(&operation.id, &deployment_id);

    // The deployment row exists and no quarantined copy is present, but nothing
    // is live in the profile.
    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "INSTALL_DEPLOYMENT_MISSING");
    assert_eq!(error.category, AppErrorCategory::Recovery);
    let persisted = h.operation(&operation.id);
    assert_eq!(persisted.state, OperationState::RecoveryRequired);
    assert!(!h.mods_dir().join("Example").exists());
}

/// Ownership must not win over ambiguous filesystem evidence.
#[test]
fn an_owned_install_with_both_copies_is_ambiguous_not_successful() {
    let h = harness();
    let hash = h.save_artifact_and_acquisition();
    let (deployment_id, _) = h.own_deployment("Example");
    let plan_json = h.install_plan_json(&hash, "Example", Vec::new());
    let operation = h.install_operation("Example", plan_json, OperationState::Committing);
    h.publish_folder("Example");
    h.quarantine_folder(&operation.id, "Example");
    h.save_operation(
        &operation,
        &[
            (
                4,
                OperationStepKind::PublishDeployment,
                OperationStepState::Completed,
            ),
            (
                5,
                OperationStepKind::CommitInstallDatabase,
                OperationStepState::Completed,
            ),
        ],
    );
    h.rewrite_publish_step(&operation.id, &deployment_id);

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "INSTALL_EVIDENCE_AMBIGUOUS");
    assert_eq!(
        h.operation(&operation.id).state,
        OperationState::RecoveryRequired
    );
    assert!(h.mods_dir().join("Example").exists());
    assert!(h.recovery_dir(&operation.id, "Example").exists());
}

/// A deployment at the same relative path, belonging to a different artifact,
/// is not evidence that this operation's atomic commit happened.
#[test]
fn an_unrelated_deployment_at_the_same_path_is_never_ownership_proof() {
    let h = harness();
    // The live folder belongs to a different artifact than this plan expects.
    let (unrelated_id, _) = h.own_deployment("Example");
    let plan_hash = h.save_artifact_for(&"c".repeat(64));
    let plan_json = h.install_plan_json(plan_hash.as_str(), "Example", Vec::new());
    let operation = h.install_operation("Example", plan_json, OperationState::Committing);
    h.publish_folder("Example");
    h.save_operation(
        &operation,
        &[(
            4,
            OperationStepKind::PublishDeployment,
            OperationStepState::Running,
        )],
    );
    h.rewrite_publish_step(&operation.id, &unrelated_id);

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "INSTALL_PATH_OCCUPIED");
    let persisted = h.operation(&operation.id);
    assert_ne!(
        persisted.state,
        OperationState::Succeeded,
        "an unrelated deployment at the same path must never prove this operation succeeded"
    );
    assert_eq!(persisted.state, OperationState::RecoveryRequired);
    // The unrelated deployment is untouched.
    assert_eq!(
        h.repo
            .list_deployments_for_profile(&h.profile.id)
            .unwrap()
            .len(),
        1
    );
}

/// The inverse of the install rule: a committed removal whose folder is still
/// live is not a success.
#[test]
fn a_committed_removal_with_a_still_live_folder_is_not_a_success() {
    let h = harness();
    let folder = "Example";
    let (deployment_id, component_ids) = h.own_deployment(folder);
    let plan_json = h.removal_plan_json(deployment_id, folder, &component_ids);
    let operation = h.removal_operation(plan_json, OperationState::Committing);
    h.save_operation(
        &operation,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Completed,
        )],
    );
    // The database says the removal committed...
    let mut deployment = h
        .repo
        .get_deployment(&deployment_id)
        .unwrap()
        .expect("deployment");
    deployment.state = DeploymentState::Quarantined;
    h.repo.save_deployment(&deployment).unwrap();
    for component in h.repo.list_profile_components(&h.profile.id).unwrap() {
        h.repo.delete_profile_component(&component.id).unwrap();
    }
    // ...while the folder it removed is still live in the profile.
    h.publish_folder(folder);

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "REMOVAL_EVIDENCE_INCONSISTENT");
    assert_eq!(error.category, AppErrorCategory::Recovery);
    assert_eq!(
        h.operation(&operation.id).state,
        OperationState::RecoveryRequired
    );
    assert!(h.mods_dir().join(folder).exists());
}

/// Corrupted persisted plan data must fail before anything is mutated.
#[test]
fn a_corrupted_removal_plan_fails_before_it_mutates_anything() {
    let h = harness();
    let folder = "Example";
    let (deployment_id, _) = h.own_deployment(folder);
    let plan_json = serde_json::json!({
        "profile_id": h.profile.id.to_string(),
        "deployment_id": deployment_id.to_string(),
        "deployment_rel_path": folder,
        "removed_profile_component_ids": ["not-a-uuid"],
    })
    .to_string();
    let operation = h.removal_operation(plan_json, OperationState::Committing);
    h.save_operation(
        &operation,
        &[(
            1,
            OperationStepKind::QuarantineDeployment,
            OperationStepState::Completed,
        )],
    );
    h.quarantine_folder(&operation.id, folder);

    let error = h.service.retry_recovery().unwrap_err();

    assert_eq!(error.code, "REMOVAL_PLAN_INVALID");
    assert_eq!(
        h.operation(&operation.id).state,
        OperationState::RecoveryRequired
    );
    // Nothing was partially removed: the database still owns the deployment and
    // its component.
    assert_eq!(
        h.repo.list_profile_components(&h.profile.id).unwrap().len(),
        1
    );
    assert_eq!(
        h.repo
            .get_deployment(&deployment_id)
            .unwrap()
            .unwrap()
            .state,
        DeploymentState::Present
    );
}

// ---------------------------------------------------------------------------
// Atomic journal: the commit step and the terminal state share a transaction
// ---------------------------------------------------------------------------

#[test]
fn the_commit_step_is_written_by_the_same_transaction_as_the_operation() {
    let h = harness();
    let deployment_id = DeploymentId::new();
    let target = "Example";
    let hash = h.save_artifact_and_acquisition();
    let plan_json = h.install_plan_json(&hash, target, Vec::new());
    let operation = h.install_operation(target, plan_json, OperationState::Committing);
    // The journal entry deliberately does not exist yet: the transaction is what
    // must create it, so a crash can never leave the domain commit unrecorded.
    h.save_operation(&operation, &[]);
    h.publish_folder(target);

    let commit = h.sample_install_commit(&operation, deployment_id, target, h.profile.revision);
    h.repo.commit_install(commit).expect("atomic commit");

    assert_eq!(h.operation(&operation.id).state, OperationState::Succeeded);
    assert_eq!(
        h.step(&operation.id, OperationStepKind::CommitInstallDatabase),
        Some(OperationStepState::Completed)
    );
    assert!(h
        .repo
        .list_deployments_for_profile(&h.profile.id)
        .unwrap()
        .iter()
        .any(|deployment| deployment.id == deployment_id));
}

#[test]
fn a_rejected_commit_leaves_the_step_untouched_and_the_operation_unfinished() {
    let h = harness();
    let deployment_id = DeploymentId::new();
    let target = "Example";
    let hash = h.save_artifact_and_acquisition();
    let plan_json = h.install_plan_json(&hash, target, Vec::new());
    let operation = h.install_operation(target, plan_json, OperationState::Committing);
    h.save_operation(
        &operation,
        &[(
            5,
            OperationStepKind::CommitInstallDatabase,
            OperationStepState::Running,
        )],
    );

    // The plan was validated against a revision the database has moved past.
    let commit = h.sample_install_commit(&operation, deployment_id, target, h.profile.revision + 1);
    let error = h.repo.commit_install(commit).unwrap_err();
    assert_eq!(error.code, "PROFILE_REVISION_MISMATCH");

    // The whole transaction rolled back: no domain rows, no journal claim, and
    // the operation is still mid-commit for recovery to find.
    assert!(h
        .repo
        .list_deployments_for_profile(&h.profile.id)
        .unwrap()
        .is_empty());
    assert!(h
        .repo
        .list_profile_components(&h.profile.id)
        .unwrap()
        .is_empty());
    assert_eq!(h.operation(&operation.id).state, OperationState::Committing);
    assert_eq!(
        h.step(&operation.id, OperationStepKind::CommitInstallDatabase),
        Some(OperationStepState::Running)
    );
}

// ---------------------------------------------------------------------------
// Cleanup evidence
// ---------------------------------------------------------------------------

#[test]
fn a_failed_cleanup_is_recorded_without_failing_the_install() {
    let h = harness();
    let zip = h.paths.packages_dir().join("Example.zip");
    std::fs::create_dir_all(h.paths.packages_dir()).unwrap();
    write_mod_zip(&zip);

    let mods = h.mods_service();
    let preview = mods
        .prepare_install(&h.profile.id, &zip)
        .expect("prepare install");
    let operation_id = manager_core::ids::OperationId::from_str(&preview.operation_id).unwrap();

    // Staging cleanup fails after the installation is durably committed.
    h.faults.fail_cleanup.store(true, Ordering::SeqCst);
    let dto = h
        .service
        .commit_operation(&operation_id)
        .expect("a failed cleanup must not fail the installation");

    assert_eq!(dto.state, "succeeded");
    assert_eq!(h.operation(&operation_id).state, OperationState::Succeeded);
    // The journal records what actually happened instead of claiming success.
    assert_eq!(
        h.step(&operation_id, OperationStepKind::CleanupStaging),
        Some(OperationStepState::Failed)
    );
    assert_eq!(
        h.step(&operation_id, OperationStepKind::CommitInstallDatabase),
        Some(OperationStepState::Completed)
    );
}

#[test]
fn a_successful_install_records_every_execution_boundary() {
    let h = harness();
    let zip = h.paths.packages_dir().join("Example.zip");
    std::fs::create_dir_all(h.paths.packages_dir()).unwrap();
    write_mod_zip(&zip);

    let mods = h.mods_service();
    let preview = mods
        .prepare_install(&h.profile.id, &zip)
        .expect("prepare install");
    let operation_id = manager_core::ids::OperationId::from_str(&preview.operation_id).unwrap();

    let dto = h.service.commit_operation(&operation_id).expect("install");
    assert_eq!(dto.state, "succeeded");

    for kind in [
        OperationStepKind::RetainArtifact,
        OperationStepKind::InspectAndStage,
        OperationStepKind::VerifyStaged,
        OperationStepKind::PublishDeployment,
        OperationStepKind::CommitInstallDatabase,
        OperationStepKind::CleanupStaging,
    ] {
        assert_eq!(
            h.step(&operation_id, kind),
            Some(OperationStepState::Completed),
            "{kind:?} must be recorded as completed"
        );
    }
    let deployments = h.repo.list_deployments_for_profile(&h.profile.id).unwrap();
    assert_eq!(deployments.len(), 1);
    assert!(
        h.mods_dir()
            .join(&deployments[0].root_relative_path)
            .exists(),
        "the recorded deployment must be live in the profile"
    );
    assert_eq!(
        h.operation(&operation_id).plan_schema_version,
        OPERATION_PLAN_SCHEMA_V2
    );
}

// ---------------------------------------------------------------------------
// Durable resource ownership after a restart
// ---------------------------------------------------------------------------

fn unresolved_smapi_operation(h: &Harness) -> Operation {
    use manager_core::operation::{AccessMode, OperationResource, ResourceKind};

    let operation = Operation {
        id: OperationId::new(),
        kind: OperationKind::SmapiSetup,
        state: OperationState::RecoveryRequired,
        game_installation_id: Some(h.game_id),
        profile_id: None,
        expected_profile_revision: None,
        plan_schema_version: OPERATION_PLAN_SCHEMA_V2,
        plan_json: serde_json::json!({ "release_policy_id": "pinned" }).to_string(),
        progress_current: None,
        progress_total: None,
        error_code: Some("RECONCILIATION_REQUIRED".to_string()),
        error_json: None,
        cancellation_requested: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        completed_at: None,
    };
    h.save_operation(&operation, &[]);
    // The durable declaration is what survives the restart.
    h.repo
        .save_operation_resource(&OperationResource {
            operation_id: operation.id,
            resource_kind: ResourceKind::GameInstallation,
            resource_id: h.game_id.to_string(),
            access_mode: AccessMode::Write,
        })
        .unwrap();
    operation
}

#[test]
fn an_unresolved_smapi_setup_blocks_launch_after_a_restart() {
    let h = harness();
    unresolved_smapi_operation(&h);

    let preflight = h
        .launch_service()
        .get_launch_preflight(&h.profile.id, manager_core::launch::LaunchMode::Vanilla)
        .expect("preflight");

    assert!(
        !preflight.can_launch,
        "an unresolved SMAPI setup owns the game installation: {:?}",
        preflight.blockers
    );
    assert!(
        preflight
            .blockers
            .iter()
            .any(|blocker| blocker.contains("unresolved")),
        "the blocker must name the unresolved operation: {:?}",
        preflight.blockers
    );
}

#[tokio::test]
async fn an_unresolved_smapi_setup_blocks_a_new_smapi_setup_after_a_restart() {
    let h = harness();
    unresolved_smapi_operation(&h);

    let error = h
        .smapi_service()
        .install_smapi(&h.game_id)
        .await
        .expect_err("an unresolved SMAPI setup must block a new one");

    assert_eq!(error.code, "RESOURCE_OPERATION_UNRESOLVED");
    assert_eq!(error.category, AppErrorCategory::OperationConflict);
    assert_eq!(
        error.recoverability,
        manager_app::error::Recoverability::RequiresManualIntervention
    );
}

// ---------------------------------------------------------------------------
// Recovery error semantics
// ---------------------------------------------------------------------------

#[test]
fn a_failed_recovery_state_write_reports_manual_intervention() {
    let operation_id = OperationId::new();

    let error = manager_app::error::AppError::recovery_state_persist_failed(
        operation_id,
        format!("operation {operation_id}: compensation failed; journal write failed"),
    );

    assert_eq!(error.code, "RECOVERY_STATE_PERSIST_FAILED");
    assert_eq!(error.category, AppErrorCategory::Recovery);
    assert_eq!(
        error.recoverability,
        manager_app::error::Recoverability::RequiresManualIntervention,
        "an error that asks for manual reconciliation must say so"
    );
    assert_eq!(
        error.operation_id.as_deref(),
        Some(operation_id.to_string().as_str())
    );
    assert!(error
        .technical_details
        .as_deref()
        .is_some_and(|details| details.contains("compensation failed")));
}

/// A persisted removal plan this build cannot fully read must fail before any
/// live side effect, never execute as a partial removal.
#[test]
fn a_corrupted_removal_plan_fails_before_any_live_side_effect() {
    let cases: [(&str, serde_json::Value, &str); 4] = [
        (
            "an invalid id",
            serde_json::json!(["not-a-uuid"]),
            "removed component id 'not-a-uuid' is invalid",
        ),
        (
            "a non-string id",
            serde_json::json!([7]),
            "a removed component id is not a string",
        ),
        (
            "an empty list",
            serde_json::json!([]),
            "the plan lists no components to remove",
        ),
        (
            "a missing list",
            serde_json::Value::Null,
            "the plan does not list removed_profile_component_ids",
        ),
    ];

    for (case, ids, expected_detail) in cases {
        let h = harness();
        let folder = "Example";
        let (deployment_id, component_ids) = h.own_deployment(folder);
        h.publish_folder(folder);

        let mut plan = serde_json::json!({
            "profile_id": h.profile.id.to_string(),
            "deployment_id": deployment_id.to_string(),
            "deployment_rel_path": folder,
            "removed_profile_component_ids": component_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>(),
        });
        if ids.is_null() {
            plan.as_object_mut()
                .expect("plan object")
                .remove("removed_profile_component_ids");
        } else {
            plan["removed_profile_component_ids"] = ids;
        }

        let operation = h.removal_operation(plan.to_string(), OperationState::Draft);
        h.save_operation(&operation, &[]);

        let error = h
            .service
            .commit_operation(&operation.id)
            .expect_err(&format!("a corrupted removal plan must fail: {case}"));

        assert_eq!(error.code, "REMOVAL_PLAN_INVALID", "case: {case}");
        assert_eq!(
            error.category,
            AppErrorCategory::OperationConflict,
            "case: {case}"
        );
        assert_eq!(
            error.recoverability,
            manager_app::error::Recoverability::RetryWithFreshPlan,
            "case: {case}"
        );
        assert!(
            error
                .technical_details
                .as_deref()
                .is_some_and(|details| details.contains(expected_detail)),
            "case {case} must explain what could not be read: {:?}",
            error.technical_details
        );

        // The live deployment is untouched and no quarantine copy was created.
        assert!(
            h.mods_dir().join(folder).join("Example.dll").exists(),
            "case {case}: the live deployment must remain untouched"
        );
        assert!(
            !h.recovery_dir(&operation.id, folder).exists(),
            "case {case}: nothing may be quarantined"
        );

        // The database is unchanged, and the operation never entered a live
        // side-effect step or the mutation lifecycle.
        let components = h.repo.list_profile_components(&h.profile.id).unwrap();
        assert_eq!(components.len(), 1, "case: {case}");
        assert_eq!(components[0].id, component_ids[0], "case: {case}");
        assert_eq!(
            h.repo
                .get_deployment(&deployment_id)
                .unwrap()
                .unwrap()
                .state,
            DeploymentState::Present,
            "case: {case}"
        );
        assert!(
            h.repo
                .list_operation_steps(&operation.id)
                .unwrap()
                .is_empty(),
            "case {case}: no execution step may be started"
        );
        let persisted = h.operation(&operation.id);
        assert_eq!(persisted.state, OperationState::Draft, "case: {case}");
        assert!(persisted.error_code.is_none(), "case: {case}");
    }
}

/// A well-formed plan still executes through the shared decoder.
#[test]
fn a_well_formed_removal_plan_still_executes() {
    let h = harness();
    let folder = "Example";
    let (deployment_id, component_ids) = h.own_deployment(folder);
    let plan_json = h.removal_plan_json(deployment_id, folder, &component_ids);
    let operation = h.removal_operation(plan_json, OperationState::Draft);
    h.save_operation(&operation, &[]);
    h.publish_folder(folder);

    let dto = h.service.commit_operation(&operation.id).expect("removal");

    assert_eq!(dto.state, "succeeded");
    assert!(!h.mods_dir().join(folder).exists());
    assert!(h.recovery_dir(&operation.id, folder).exists());
    assert!(h
        .repo
        .list_profile_components(&h.profile.id)
        .unwrap()
        .is_empty());
}

/// A failure the executor does not handle, after the operation has entered its
/// mutation lifecycle, is promoted to recovery with its diagnosis intact.
#[test]
fn a_failure_after_entering_the_mutation_phase_is_promoted_with_its_diagnosis() {
    let h = harness();
    let zip = h.paths.packages_dir().join("Example.zip");
    std::fs::create_dir_all(h.paths.packages_dir()).unwrap();
    write_mod_zip(&zip);

    let preview = h
        .mods_service()
        .prepare_install(&h.profile.id, &zip)
        .expect("prepare install");
    let operation_id = manager_core::ids::OperationId::from_str(&preview.operation_id).unwrap();

    // Execution can no longer prepare its staging tree, which fails after the
    // operation has been moved into the mutation phase.
    h.faults.fail_create_staging.store(true, Ordering::SeqCst);
    let error = h
        .service
        .commit_operation(&operation_id)
        .expect_err("a staging failure must fail the commit");

    let persisted = h.operation(&operation_id);
    assert_eq!(persisted.state, OperationState::RecoveryRequired);
    assert_eq!(
        persisted.error_code.as_deref(),
        Some("EXECUTION_INTERRUPTED"),
        "the fallback must record why the operation was promoted"
    );
    assert_eq!(error.category, AppErrorCategory::Recovery);
    assert_eq!(
        error.recoverability,
        manager_app::error::Recoverability::RequiresManualIntervention
    );
    assert_eq!(
        error.operation_id.as_deref(),
        Some(operation_id.to_string().as_str())
    );
}
