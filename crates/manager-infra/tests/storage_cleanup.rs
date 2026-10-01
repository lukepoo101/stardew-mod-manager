//! Storage cleanup removes only manager-owned items nothing still needs, and
//! reports every item it was asked to remove.

use chrono::Utc;
use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, OperationRepository,
    PackageCatalogRepository, PreferencesRepository, ProfileRepository,
};
use manager_app::services::{ResourceClaim, ResourceCoordinator, StorageCleanupService};
use manager_core::deployment::{DeploymentState, ProfileDeployment};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{ArtifactHash, DeploymentId, OperationId};
use manager_core::operation::{Operation, OperationKind, OperationState, ResourceKind};
use manager_core::package::PackageArtifact;
use manager_core::ports::InstanceLock;
use manager_core::profile::{Profile, ProfileState};
use manager_infra::db::SqliteStateRepository;
use manager_infra::paths::AppPaths;
use manager_infra::FilesystemStorageInventory;
use std::path::PathBuf;
use std::sync::Arc;

struct NoLock;
impl InstanceLock for NoLock {
    fn acquire_guard(&self) -> Result<Box<dyn std::any::Any + Send + Sync>, String> {
        Ok(Box::new(()))
    }
}

struct Fixture {
    repo: Arc<SqliteStateRepository>,
    paths: AppPaths,
    resources: Arc<ResourceCoordinator>,
    service: StorageCleanupService,
    profile: Profile,
    game: GameInstallation,
    _tmp: tempfile::TempDir,
}

fn hash(c: char) -> String {
    c.to_string().repeat(64)
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("s.sqlite3")).unwrap());
        let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
        paths.ensure_directories().unwrap();
        let game = GameInstallation {
            id: manager_core::ids::GameInstallationId::new(),
            canonical_root: tmp.path().join("Game"),
            operating_system: OperatingSystem::Linux,
            storefront: Storefront::Steam,
            management_mode: ManagementMode::Managed,
            created_at: Utc::now(),
        };
        repo.save_game(&game).unwrap();
        let profile = Profile::new(game.id, "Co-op");
        repo.save_profile(&profile).unwrap();
        let resources = Arc::new(ResourceCoordinator::new());
        let service = StorageCleanupService::new(
            resources.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            Arc::new(FilesystemStorageInventory::new(
                paths.data_dir().to_path_buf(),
                paths.packages_dir(),
                paths.smapi_cache_dir(),
            )),
            Arc::new(NoLock),
        )
        .with_recovery_references(repo.clone());
        Self {
            repo,
            paths,
            resources,
            service,
            profile,
            game,
            _tmp: tmp,
        }
    }

    fn package(&self, c: char, bytes: usize) -> PathBuf {
        let path = self.paths.package_path(&hash(c));
        std::fs::write(&path, vec![0u8; bytes]).unwrap();
        path
    }

    fn use_package(&self, profile: &Profile, c: char) {
        let artifact_hash = ArtifactHash::parse(hash(c)).unwrap();
        self.repo
            .save_artifact(&PackageArtifact {
                hash: artifact_hash.clone(),
                byte_size: 1,
                storage_relative_path: format!("packages/{}.zip", hash(c)),
                first_seen_at: Utc::now(),
            })
            .unwrap();
        self.repo
            .save_deployment(&ProfileDeployment {
                id: DeploymentId::new(),
                profile_id: profile.id,
                artifact_hash,
                root_relative_path: format!("Mod{c}"),
                installed_at: Utc::now(),
                state: DeploymentState::Present,
            })
            .unwrap();
    }

    fn operation(&self, state: OperationState) -> OperationId {
        let now = Utc::now();
        let op = Operation {
            id: OperationId::new(),
            kind: OperationKind::ModInstall,
            state,
            game_installation_id: Some(self.game.id),
            profile_id: Some(self.profile.id),
            expected_profile_revision: None,
            plan_schema_version: 1,
            plan_json: "{}".into(),
            progress_current: None,
            progress_total: None,
            error_code: None,
            error_json: None,
            cancellation_requested: false,
            created_at: now,
            updated_at: now,
            completed_at: state.is_terminal().then_some(now),
        };
        self.repo.create_operation(&op).unwrap();
        op.id
    }

    fn leftover(&self, folder: &str, op: &str) -> PathBuf {
        let dir = self
            .paths
            .data_dir()
            .join("setups")
            .join(self.profile.id.to_string())
            .join(folder)
            .join(op);
        std::fs::create_dir_all(dir.join("Mod")).unwrap();
        std::fs::write(dir.join("Mod").join("file.dll"), vec![0u8; 10]).unwrap();
        dir
    }

    fn item<'a>(
        preview: &'a manager_app::api::dto::CleanupPreviewDto,
        prefix: &str,
    ) -> &'a manager_app::api::dto::CleanupItemDto {
        preview
            .items
            .iter()
            .find(|item| item.id.starts_with(prefix))
            .unwrap_or_else(|| panic!("no item {prefix} in {:#?}", preview.items))
    }
}

#[test]
fn packages_used_by_any_profile_are_protected_and_unused_ones_are_reclaimable() {
    let f = Fixture::new();
    f.package('a', 100);
    f.package('b', 50);
    f.package('c', 20);
    f.use_package(&f.profile, 'a');
    // An archived profile still owns its packages.
    let mut archived = Profile::new(f.game.id, "Old save");
    archived.state = ProfileState::Archived;
    f.repo.save_profile(&archived).unwrap();
    f.use_package(&archived, 'b');

    let preview = f.service.preview().unwrap();
    let used = Fixture::item(&preview, &format!("package:{}", hash('a')));
    assert!(!used.removable);
    assert_eq!(used.detail, "Used by Co-op.");
    assert!(!Fixture::item(&preview, &format!("package:{}", hash('b'))).removable);
    let unused = Fixture::item(&preview, &format!("package:{}", hash('c')));
    assert!(unused.removable);
    assert_eq!(unused.category, "unused_package");
    assert_eq!(preview.reclaimable_bytes, 20);
    assert_eq!(preview.protected_bytes, 150);
    assert!(preview.blocked_reason.is_none());
}

#[test]
fn running_removes_only_requested_items_that_are_still_removable() {
    let f = Fixture::new();
    let unused = f.package('c', 20);
    let used = f.package('a', 100);
    f.use_package(&f.profile, 'a');
    let cache = f.paths.smapi_cache_dir().join("extracted_installer");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("installer"), vec![0u8; 7]).unwrap();

    let result = f
        .service
        .run(&[
            format!("package:{}", hash('c')),
            format!("package:{}", hash('a')),
            "smapi-cache:extracted_installer".into(),
            "package:does-not-exist".into(),
        ])
        .unwrap();

    assert!(!unused.exists());
    assert!(!cache.exists());
    assert!(used.exists(), "a package in use must never be removed");
    assert_eq!(result.reclaimed_bytes, 27);
    assert!(!result.complete);
    let outcome = |id: &str| {
        result
            .outcomes
            .iter()
            .find(|o| o.id.starts_with(id))
            .map(|o| o.outcome.as_str())
            .unwrap()
    };
    assert_eq!(outcome(&format!("package:{}", hash('c'))), "removed");
    assert_eq!(outcome(&format!("package:{}", hash('a'))), "skipped");
    assert_eq!(outcome("package:does-not-exist"), "skipped");

    // Repeating the cleanup is harmless: the removed items are simply gone.
    let again = f.service.preview().unwrap();
    assert_eq!(again.reclaimable_bytes, 0);
}

#[test]
fn leftovers_of_finished_changes_are_reclaimable_but_unfinished_ones_are_kept() {
    let f = Fixture::new();
    let finished = f.operation(OperationState::Succeeded);
    let finished_dir = f.leftover(".recovery", &finished.to_string());
    let orphan_dir = f.leftover(".staging", &OperationId::new().to_string());

    let preview = f.service.preview().unwrap();
    assert!(Fixture::item(&preview, "recovery:").removable);
    assert!(Fixture::item(&preview, "staging:").removable);
    assert_eq!(preview.reclaimable_bytes, 20);

    let pending = f.operation(OperationState::RecoveryRequired);
    let pending_dir = f.leftover(".staging", &pending.to_string());
    f.package('c', 5);
    let preview = f.service.preview().unwrap();
    assert!(preview.blocked_reason.is_some());
    assert!(
        preview.items.iter().all(|item| !item.removable),
        "an unresolved change keeps every package and leftover: {:#?}",
        preview.items
    );

    let ids: Vec<String> = preview.items.iter().map(|item| item.id.clone()).collect();
    let result = f.service.run(&ids).unwrap();
    assert!(result.outcomes.iter().all(|o| o.outcome == "skipped"));
    assert!(finished_dir.exists() && orphan_dir.exists() && pending_dir.exists());
}

#[test]
fn another_running_task_blocks_the_cleanup() {
    let f = Fixture::new();
    f.package('c', 5);
    let _held = f
        .resources
        .try_acquire(&[ResourceClaim::write(
            ResourceKind::Profile,
            f.profile.id.to_string(),
        )])
        .unwrap();
    assert!(f.service.preview().unwrap().blocked_reason.is_some());
    assert!(f.service.run(&[format!("package:{}", hash('c'))]).is_err());
    assert!(f.paths.package_path(&hash('c')).exists());
}

#[test]
fn nothing_selected_is_rejected() {
    let f = Fixture::new();
    assert!(f.service.run(&[]).is_err());
}

#[cfg(unix)]
#[test]
fn links_are_never_followed_or_removed() {
    let f = Fixture::new();
    let outside = f._tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("precious"), vec![0u8; 1000]).unwrap();
    std::os::unix::fs::symlink(&outside, f.paths.smapi_cache_dir().join("linked")).unwrap();
    // A link inside a leftover folder is removed as a link, not followed.
    let op = f.operation(OperationState::Succeeded);
    let dir = f.leftover(".recovery", &op.to_string());
    std::os::unix::fs::symlink(&outside, dir.join("escape")).unwrap();

    let preview = f.service.preview().unwrap();
    let linked = Fixture::item(&preview, "smapi-cache:linked");
    assert!(!linked.removable);
    assert_eq!(linked.category, "protected");
    // The link's target is not counted towards the leftover's size.
    assert_eq!(Fixture::item(&preview, "recovery:").size_bytes, 10);

    let ids: Vec<String> = preview.items.iter().map(|item| item.id.clone()).collect();
    f.service.run(&ids).unwrap();
    assert!(outside.join("precious").exists());
    assert!(f.paths.smapi_cache_dir().join("linked").exists());
    assert!(!dir.exists());
}

#[cfg(unix)]
#[test]
fn a_failed_removal_is_reported_per_item() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.package('c', 5);
    let cache = f.paths.smapi_cache_dir().join("stuck");
    std::fs::create_dir_all(cache.join("inner")).unwrap();
    std::fs::write(cache.join("inner").join("file"), b"x").unwrap();
    std::fs::set_permissions(cache.join("inner"), std::fs::Permissions::from_mode(0o500)).unwrap();

    let result = f
        .service
        .run(&[format!("package:{}", hash('c')), "smapi-cache:stuck".into()])
        .unwrap();
    std::fs::set_permissions(cache.join("inner"), std::fs::Permissions::from_mode(0o700)).unwrap();

    // Running as root ignores the permission, so only check the report shape
    // when the removal really failed.
    if cache.exists() {
        assert!(!result.complete);
        let stuck = result
            .outcomes
            .iter()
            .find(|o| o.id == "smapi-cache:stuck")
            .unwrap();
        assert_eq!(stuck.outcome, "failed");
        assert!(stuck.message.is_some());
    }
    let package = result
        .outcomes
        .iter()
        .find(|o| o.id.starts_with("package:"))
        .unwrap();
    assert_eq!(package.outcome, "removed");
}

#[test]
fn retention_keeps_the_newest_backups_and_recent_trash() {
    let f = Fixture::new();
    let data = f.paths.data_dir();
    // Seven backups of one save: the five newest are kept.
    for day in 1..=7 {
        let dir = data
            .join("save-backups")
            .join("Farm_1")
            .join(format!("202609{day:02}T100000000"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Farm_1"), vec![0u8; 10]).unwrap();
    }
    // A backup still being written is never listed.
    std::fs::create_dir_all(data.join("save-backups/Farm_1/20260908T100000000.part")).unwrap();
    // Trash: one long gone, one recent.
    let old = data.join("trash/profile-abc-20200101T000000");
    let recent = data.join(format!(
        "trash/profile-def-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S")
    ));
    for dir in [&old, &recent] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("x"), b"x").unwrap();
    }

    let preview = f.service.preview().unwrap();
    let saves: Vec<_> = preview
        .items
        .iter()
        .filter(|i| i.id.starts_with("save-backup:"))
        .collect();
    assert_eq!(saves.len(), 7);
    let mut removable: Vec<&str> = saves
        .iter()
        .filter(|i| i.removable)
        .map(|i| i.id.as_str())
        .collect();
    removable.sort();
    assert_eq!(
        removable,
        vec![
            "save-backup:Farm_1:20260901T100000000",
            "save-backup:Farm_1:20260902T100000000"
        ]
    );
    let trash = |name: &str| {
        preview
            .items
            .iter()
            .find(|i| i.id.contains(name))
            .unwrap()
            .removable
    };
    assert!(trash("profile-abc"));
    assert!(!trash("profile-def"));

    let ids: Vec<String> = preview
        .items
        .iter()
        .filter(|i| i.removable)
        .map(|i| i.id.clone())
        .collect();
    let result = f.service.run(&ids).unwrap();
    assert!(result.complete, "{:?}", result.outcomes);
    assert!(!data.join("save-backups/Farm_1/20260901T100000000").exists());
    assert!(data.join("save-backups/Farm_1/20260903T100000000").exists());
    assert!(!old.exists());
    assert!(recent.exists());
}

#[test]
fn packages_a_restore_point_or_last_working_setup_needs_are_kept() {
    let f = Fixture::new();
    f.package('d', 30);
    f.package('e', 40);
    f.package('f', 5);
    let frozen = |c: char| {
        serde_json::json!({
            "unique_id": format!("Me.Mod{c}"),
            "name": format!("Mod {c}"),
            "version": "1.0.0",
            "artifact_hash": hash(c),
            "enabled": true,
        })
    };
    f.repo
        .set_preference(
            &format!("restore_points:{}", f.profile.id),
            &serde_json::json!([{
                "id": "p1",
                "label": "Before update",
                "created_at": Utc::now().to_rfc3339(),
                "mods": [frozen('d')],
            }])
            .to_string(),
        )
        .unwrap();
    f.repo
        .set_preference(
            &format!("known_good:{}", f.profile.id),
            &serde_json::json!({
                "profile_id": f.profile.id.to_string(),
                "recorded_at": Utc::now().to_rfc3339(),
                "game_version": null,
                "smapi_version": null,
                "mods": [frozen('e')],
            })
            .to_string(),
        )
        .unwrap();

    let preview = f.service.preview().unwrap();
    let for_point = Fixture::item(&preview, &format!("package:{}", hash('d')));
    assert!(!for_point.removable);
    assert_eq!(
        for_point.detail,
        "Used by restore point \"Before update\" of Co-op."
    );
    let for_known_good = Fixture::item(&preview, &format!("package:{}", hash('e')));
    assert!(!for_known_good.removable);
    assert_eq!(
        for_known_good.detail,
        "Used by the last working setup of Co-op."
    );
    assert!(Fixture::item(&preview, &format!("package:{}", hash('f'))).removable);
}

#[test]
fn a_chosen_retention_policy_changes_what_is_offered_and_persists() {
    let f = Fixture::new();
    let data = f.paths.data_dir();
    for day in 1..=4 {
        let dir = data
            .join("save-backups")
            .join("Farm_1")
            .join(format!("202609{day:02}T100000000"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Farm_1"), vec![0u8; 10]).unwrap();
    }
    let removable_saves = |service: &StorageCleanupService| {
        service
            .preview()
            .unwrap()
            .items
            .iter()
            .filter(|i| i.id.starts_with("save-backup:") && i.removable)
            .count()
    };
    assert_eq!(f.service.retention().unwrap().keep_save_backups, 5);
    assert_eq!(removable_saves(&f.service), 0);

    let policy = manager_app::api::dto::RetentionPolicyDto {
        keep_save_backups: 2,
        keep_settings_backups: 5,
        keep_trash_days: 7,
    };
    f.service.set_retention(policy).unwrap();
    assert_eq!(removable_saves(&f.service), 2);
    // The policy is stored, so it holds after a restart.
    assert_eq!(f.service.retention().unwrap(), policy);

    let error = f
        .service
        .set_retention(manager_app::api::dto::RetentionPolicyDto {
            keep_save_backups: 0,
            ..policy
        })
        .unwrap_err();
    assert_eq!(error.code, "RETENTION_OUT_OF_RANGE");
    assert_eq!(f.service.retention().unwrap(), policy);
}
