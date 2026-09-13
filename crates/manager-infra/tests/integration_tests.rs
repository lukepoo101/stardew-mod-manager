use manager_core::domain::*;
use manager_core::install::InstallPlan;
use manager_core::ports::*;
use manager_core::use_cases::CoreUseCases;
use manager_infra::archive::SafeZipExtractor;
use manager_infra::db::SqliteStateRepository;
use manager_infra::launcher::DetachedGameLauncher;
use manager_infra::lock::FileInstanceLock;
use manager_infra::log_reader::SmapiSessionLogReader;
use manager_infra::package_store::FilesystemPackageStore;
use manager_infra::smapi_adapter::ProcessSmapiInstaller;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn create_synthetic_mod_zip(
    path: &std::path::Path,
    manifest_json: &str,
    extra_files: &[(&str, &[u8])],
) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(manifest_json.as_bytes()).unwrap();

    for (name, content) in extra_files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(content).unwrap();
    }

    zip.finish().unwrap();
}

#[test]
fn test_valid_mod_lifecycle_end_to_end() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let pkg_store = FilesystemPackageStore::new(root.join("packages"));
    let smapi_installer = ProcessSmapiInstaller::new(root.join("smapi_cache"));
    let launcher = DetachedGameLauncher::isolated();
    let log_reader = SmapiSessionLogReader::new(Some(root.join("logs/SMAPI-latest.txt")));
    let lock = FileInstanceLock::new(root.join(".instance.lock"));

    let use_cases = CoreUseCases::new(repo, pkg_store, smapi_installer, launcher, log_reader, lock);

    // 1. Create a mock game directory
    let game_dir = root.join("StardewValley");
    std::fs::create_dir_all(&game_dir).unwrap();
    File::create(game_dir.join("Stardew Valley.dll")).unwrap();
    File::create(game_dir.join("StardewValley")).unwrap();

    let game = use_cases
        .inspect_game(&game_dir, StoreKind::SteamNative)
        .unwrap();
    assert!(game.is_fresh);
    let game = use_cases.accept_game(&game).unwrap();

    let snapshot = use_cases.get_app_snapshot(Some(&game.id)).unwrap();
    let setup = snapshot.setup.unwrap();

    // 2. Create a synthetic mod archive
    let mod_zip_path = root.join("TestMod.zip");
    let manifest = r#"{
        "Name": "Example Mod",
        "Author": "Tester",
        "Version": "1.0.0",
        "UniqueID": "Tester.ExampleMod",
        "EntryDll": "ExampleMod.dll"
    }"#;
    create_synthetic_mod_zip(
        &mod_zip_path,
        manifest,
        &[("ExampleMod.dll", b"fake binary dll content")],
    );

    // Inspect archive
    let staging_dir = root.join("staging");
    let inspection = SafeZipExtractor::inspect_and_stage(
        &mod_zip_path,
        &setup.id,
        &staging_dir,
        &use_cases.repo,
    )
    .unwrap();

    assert_eq!(inspection.plan.manifest.name, "Example Mod");
    assert!(inspection.plan.dependency_report.is_installable);

    // Commit mod install
    let mods_dir = root.join("Mods");
    let installed_mod = use_cases
        .commit_mod_install(&inspection.plan, &staging_dir, &mods_dir)
        .unwrap();

    assert_eq!(installed_mod.name, "Example Mod");
    assert!(mods_dir.join("Tester.ExampleMod/ExampleMod.dll").exists());

    // Verify snapshot reflects installed mod
    let snapshot2 = use_cases.get_app_snapshot(Some(&game.id)).unwrap();
    assert_eq!(snapshot2.installed_mods.len(), 1);

    // Remove mod
    let recovery_dir = root.join("recovery");
    use_cases
        .remove_mod(&installed_mod.id, &setup.id, &mods_dir, &recovery_dir)
        .unwrap();

    assert!(!mods_dir.join("Tester.ExampleMod").exists());
    let snapshot3 = use_cases.get_app_snapshot(Some(&game.id)).unwrap();
    assert_eq!(snapshot3.installed_mods.len(), 0);
}

#[test]
fn test_adversarial_zip_traversal_rejected() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let repo = SqliteStateRepository::new_in_memory().unwrap();

    let bad_zip = root.join("traversal.zip");
    let file = File::create(&bad_zip).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();

    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(br#"{"Name":"Evil","Author":"Bad","Version":"1.0","UniqueID":"Evil.Mod","EntryDll":"bad.dll"}"#).unwrap();

    // Adversarial entry with directory traversal
    zip.start_file("../../../etc/shadow", options).unwrap();
    zip.write_all(b"attack payload").unwrap();
    zip.finish().unwrap();

    let staging_dir = root.join("staging");
    let res = SafeZipExtractor::inspect_and_stage(&bad_zip, "setup-1", &staging_dir, &repo);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("illegal parent traversal"));
}

#[test]
fn test_missing_entry_dll_rejected() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let repo = SqliteStateRepository::new_in_memory().unwrap();

    let zip_path = root.join("missing_dll.zip");
    let manifest = r#"{
        "Name": "Missing Dll Mod",
        "Author": "Tester",
        "Version": "1.0.0",
        "UniqueID": "Tester.MissingDll",
        "EntryDll": "NonExistent.dll"
    }"#;
    create_synthetic_mod_zip(&zip_path, manifest, &[]);

    let staging_dir = root.join("staging");
    let res = SafeZipExtractor::inspect_and_stage(&zip_path, "setup-1", &staging_dir, &repo);
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("declares EntryDll 'NonExistent.dll', but that file was not found"));
}

#[test]
fn test_missing_required_dependency_blocks_install() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let repo = SqliteStateRepository::new_in_memory().unwrap();

    let zip_path = root.join("dep_mod.zip");
    let manifest = r#"{
        "Name": "Dependent Mod",
        "Author": "Tester",
        "Version": "1.0.0",
        "UniqueID": "Tester.DependentMod",
        "EntryDll": "Dep.dll",
        "Dependencies": [
            {
                "UniqueID": "Pathoschild.ContentPatcher",
                "MinimumVersion": "2.0.0",
                "IsRequired": true
            }
        ]
    }"#;
    create_synthetic_mod_zip(&zip_path, manifest, &[("Dep.dll", b"binary")]);

    let staging_dir = root.join("staging");
    let inspection =
        SafeZipExtractor::inspect_and_stage(&zip_path, "setup-1", &staging_dir, &repo).unwrap();

    assert!(!inspection.plan.dependency_report.is_installable);
    assert_eq!(inspection.plan.dependency_report.findings.len(), 1);
    assert!(!inspection.plan.dependency_report.findings[0].satisfied);
}

#[test]
fn test_crash_recovery_resumes_cleanly() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let pkg_store = FilesystemPackageStore::new(root.join("packages"));
    let smapi_installer = ProcessSmapiInstaller::new(root.join("smapi_cache"));
    let launcher = DetachedGameLauncher::isolated();
    let log_reader = SmapiSessionLogReader::new(None);
    let lock = FileInstanceLock::new(root.join(".instance.lock"));

    let use_cases = CoreUseCases::new(repo, pkg_store, smapi_installer, launcher, log_reader, lock);

    let mods_dir = root.join("Mods");
    let staging_dir = root.join("staging");
    let recovery_dir = root.join("recovery");
    std::fs::create_dir_all(&mods_dir).unwrap();
    std::fs::create_dir_all(&staging_dir).unwrap();
    std::fs::create_dir_all(&recovery_dir).unwrap();

    // Simulate an interrupted operation where files were already moved to Mods, but crash happened before DB commit
    let plan = InstallPlan {
        plan_id: "plan-123".to_string(),
        setup_id: "setup-1".to_string(),
        package_hash: "hash123".to_string(),
        original_filename: "mod.zip".to_string(),
        mod_folder_name: "RecoveredMod".to_string(),
        manifest: Manifest {
            unique_id: "Author.Recovered".to_string(),
            name: "Recovered Mod".to_string(),
            author: "Author".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            entry_dll: None,
            minimum_api_version: None,
            dependencies: Vec::new(),
            content_pack_for: None,
        },
        raw_manifest: "{}".to_string(),
        file_inventory: vec!["file.txt".to_string()],
        trusted_inventory: vec![manager_core::install::InventoryEntry {
            relative_path: "file.txt".into(),
            entry_type: manager_core::install::InventoryEntryType::File,
            size_bytes: 0,
            sha256_hash: Some(
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
            ),
        }],
        dependency_report: manager_core::manifest::DependencyReport {
            is_installable: true,
            smapi_compatible: true,
            smapi_required_version: None,
            current_smapi_version: None,
            duplicate_id: false,
            findings: Vec::new(),
        },
        component_manifests: Vec::new(),
    };

    // Save game and setup for foreign key integrity
    let game = GameInstallation {
        id: "game-1".to_string(),
        canonical_root: root.join("game"),
        platform_kind: StoreKind::SteamNative,
        detected_version: Some("1.6".to_string()),
        validated_at: chrono::Utc::now(),
        is_fresh: true,
        is_managed: true,
        validation_error: None,
    };
    use_cases.repo.save_game(&game).unwrap();

    let setup = Setup {
        id: "setup-1".to_string(),
        game_id: "game-1".to_string(),
        display_name: "Default".to_string(),
        relative_mods_dir: "Mods".to_string(),
        created_at: chrono::Utc::now(),
    };
    use_cases.repo.save_setup(&setup).unwrap();

    // Save prepared operation
    let op = Operation {
        id: "op-interrupted".to_string(),
        kind: OperationKind::ModInstall,
        state: OperationState::Prepared,
        plan_json: serde_json::to_string(&plan).unwrap(),
        error_json: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        schema_version: 1,
    };
    use_cases.repo.save_operation(&op).unwrap();

    // Create the directory in final destination
    let final_dest = mods_dir.join("RecoveredMod");
    std::fs::create_dir_all(&final_dest).unwrap();
    File::create(final_dest.join("file.txt")).unwrap();

    // Run recovery
    let recovered_count = use_cases
        .recover_operations(&mods_dir, &staging_dir, &recovery_dir)
        .unwrap();

    assert_eq!(recovered_count, 1);

    // Verify operation is completed and mod is recorded in DB
    let op_after = use_cases
        .repo
        .get_operation("op-interrupted")
        .unwrap()
        .unwrap();
    assert_eq!(op_after.state, OperationState::Completed);

    let installed_list = use_cases.repo.list_installed_mods("setup-1").unwrap();
    assert_eq!(installed_list.len(), 1);
    assert_eq!(installed_list[0].unique_id, "Author.Recovered");
}

fn create_synthetic_smapi_installer_zip(path: &std::path::Path, script_content: &str) -> String {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    #[cfg(unix)]
    let options = SimpleFileOptions::default().unix_permissions(0o755);
    #[cfg(not(unix))]
    let options = SimpleFileOptions::default();

    zip.start_file(
        "SMAPI 4.1.10 installer/internal/linux/SMAPI.Installer",
        options,
    )
    .unwrap();
    zip.write_all(script_content.as_bytes()).unwrap();
    zip.finish().unwrap();

    let (hash, _) = SafeZipExtractor::compute_sha256(path).unwrap();
    hash
}

#[test]
fn test_smapi_installation_happy_path_and_snapshot_lifecycle() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let pkg_store = FilesystemPackageStore::new(root.join("packages"));
    let launcher = DetachedGameLauncher::isolated();
    let log_reader = SmapiSessionLogReader::new(Some(root.join("logs/SMAPI-latest.txt")));
    let lock = FileInstanceLock::new(root.join(".instance.lock"));

    // 1. Create a mock game directory
    let game_dir = root.join("StardewValley");
    std::fs::create_dir_all(&game_dir).unwrap();
    File::create(game_dir.join("Stardew Valley.dll")).unwrap();
    File::create(game_dir.join("StardewValley")).unwrap();

    // 2. Create synthetic installer zip with valid mock script
    let installer_zip = root.join("mock-smapi-installer.zip");
    let mock_script = r#"#!/bin/bash
while [[ $# -gt 0 ]]; do
  case $1 in
    --game-path)
      GAME_PATH="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [ -n "$GAME_PATH" ]; then
  mkdir -p "$GAME_PATH/smapi-internal"
  mkdir -p "$GAME_PATH/Mods/SaveBackup"
  touch "$GAME_PATH/StardewModdingAPI"
  chmod +x "$GAME_PATH/StardewModdingAPI"
  touch "$GAME_PATH/StardewModdingAPI.dll"
  touch "$GAME_PATH/StardewModdingAPI.deps.json"
  echo "SMAPI is installed!"
  exit 0
else
  echo "No game path provided" >&2
  exit 1
fi
"#;
    let installer_hash = create_synthetic_smapi_installer_zip(&installer_zip, mock_script);
    let smapi_installer =
        ProcessSmapiInstaller::new_with_expected_hash(root.join("smapi_cache"), &installer_hash);
    let use_cases = CoreUseCases::new(repo, pkg_store, smapi_installer, launcher, log_reader, lock);

    let game = use_cases
        .inspect_game(&game_dir, StoreKind::SteamNative)
        .unwrap();
    assert!(game.is_fresh);
    let game = use_cases.accept_game(&game).unwrap();

    // Initial snapshot: SMAPI should NOT be installed
    let snap_before = use_cases.get_app_snapshot(Some(&game.id)).unwrap();
    assert!(!snap_before.smapi_installed);
    assert_eq!(snap_before.smapi_version, None);

    // 3. Run SMAPI installation
    let rec = use_cases
        .install_smapi(&game.id, Some(&installer_zip))
        .unwrap();
    assert_eq!(rec.game_id, game.id);
    assert_eq!(rec.release_version, "4.1.10");

    // 4. Verify snapshot reflects installed SMAPI
    let snap_after = use_cases.get_app_snapshot(Some(&game.id)).unwrap();
    assert!(snap_after.smapi_installed);
    assert_eq!(snap_after.smapi_version.as_deref(), Some("4.1.10"));

    // 5. Verify database records
    let saved_rec = use_cases
        .repo
        .get_smapi_installation(&game.id)
        .unwrap()
        .unwrap();
    assert_eq!(saved_rec.game_id, game.id);
}

#[test]
fn test_smapi_installation_rejected_on_non_fresh_game() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let pkg_store = FilesystemPackageStore::new(root.join("packages"));
    let smapi_installer = ProcessSmapiInstaller::new(root.join("smapi_cache"));
    let launcher = DetachedGameLauncher::isolated();
    let log_reader = SmapiSessionLogReader::new(None);
    let lock = FileInstanceLock::new(root.join(".instance.lock"));

    let use_cases = CoreUseCases::new(repo, pkg_store, smapi_installer, launcher, log_reader, lock);

    // Save non-fresh game directly
    let game = GameInstallation {
        id: "game-non-fresh".to_string(),
        canonical_root: root.join("game_non_fresh"),
        platform_kind: StoreKind::SteamNative,
        detected_version: Some("1.6".to_string()),
        validated_at: chrono::Utc::now(),
        is_fresh: false,
        is_managed: false,
        validation_error: Some("Existing mods detected".to_string()),
    };
    use_cases.repo.save_game(&game).unwrap();

    let res = use_cases.install_smapi(&game.id, None);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("Game installation is not fresh"));
}

#[test]
fn test_smapi_installation_bad_path_installer_failure() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let pkg_store = FilesystemPackageStore::new(root.join("packages"));
    let launcher = DetachedGameLauncher::isolated();
    let log_reader = SmapiSessionLogReader::new(None);
    let lock = FileInstanceLock::new(root.join(".instance.lock"));

    let game_dir = root.join("StardewValley");
    std::fs::create_dir_all(&game_dir).unwrap();
    File::create(game_dir.join("Stardew Valley.dll")).unwrap();
    File::create(game_dir.join("StardewValley")).unwrap();

    // Failing installer script
    let installer_zip = root.join("failing-installer.zip");
    let mock_script = r#"#!/bin/bash
echo "Fatal: could not patch game executable" >&2
exit 42
"#;
    let installer_hash = create_synthetic_smapi_installer_zip(&installer_zip, mock_script);
    let smapi_installer =
        ProcessSmapiInstaller::new_with_expected_hash(root.join("smapi_cache"), &installer_hash);
    let use_cases = CoreUseCases::new(repo, pkg_store, smapi_installer, launcher, log_reader, lock);

    let game = use_cases
        .inspect_game(&game_dir, StoreKind::SteamNative)
        .unwrap();
    let game = use_cases.accept_game(&game).unwrap();

    let res = use_cases.install_smapi(&game.id, Some(&installer_zip));
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("42") || err.contains("Fatal: could not patch game executable"));

    // Verify snapshot: SMAPI must not be installed
    let snap = use_cases.get_app_snapshot(Some(&game.id)).unwrap();
    assert!(!snap.smapi_installed);
}

#[test]
fn test_smapi_installation_missing_artifacts_detection() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let pkg_store = FilesystemPackageStore::new(root.join("packages"));
    let launcher = DetachedGameLauncher::isolated();
    let log_reader = SmapiSessionLogReader::new(None);
    let lock = FileInstanceLock::new(root.join(".instance.lock"));

    let game_dir = root.join("StardewValley");
    std::fs::create_dir_all(&game_dir).unwrap();
    File::create(game_dir.join("Stardew Valley.dll")).unwrap();
    File::create(game_dir.join("StardewValley")).unwrap();

    // Script says "SMAPI is installed!" but fails to create StardewModdingAPI binary
    let installer_zip = root.join("incomplete-installer.zip");
    let mock_script = r#"#!/bin/bash
echo "SMAPI is installed!"
exit 0
"#;
    let installer_hash = create_synthetic_smapi_installer_zip(&installer_zip, mock_script);
    let smapi_installer =
        ProcessSmapiInstaller::new_with_expected_hash(root.join("smapi_cache"), &installer_hash);
    let use_cases = CoreUseCases::new(repo, pkg_store, smapi_installer, launcher, log_reader, lock);

    let game = use_cases
        .inspect_game(&game_dir, StoreKind::SteamNative)
        .unwrap();
    let game = use_cases.accept_game(&game).unwrap();

    let res = use_cases.install_smapi(&game.id, Some(&installer_zip));
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("SMAPI verification failed"));
}

#[test]
fn test_multi_mod_bundle_synthetic_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let pkg_store = FilesystemPackageStore::new(root.join("packages"));
    let smapi_installer = ProcessSmapiInstaller::new(root.join("smapi_cache"));
    let launcher = DetachedGameLauncher::isolated();
    let log_reader = SmapiSessionLogReader::new(None);
    let lock = FileInstanceLock::new(root.join(".instance.lock"));

    let use_cases = CoreUseCases::new(repo, pkg_store, smapi_installer, launcher, log_reader, lock);

    // Set up game and setup
    let game = GameInstallation {
        id: "game-bundle-test".to_string(),
        canonical_root: root.join("Game"),
        platform_kind: StoreKind::ManualFolder,
        detected_version: Some("1.6.14".to_string()),
        validated_at: chrono::Utc::now(),
        is_fresh: true,
        is_managed: true,
        validation_error: None,
    };
    use_cases.repo.save_game(&game).unwrap();
    let setup = Setup {
        id: "setup-bundle-test".to_string(),
        game_id: game.id.clone(),
        display_name: "Default".to_string(),
        relative_mods_dir: "Mods".to_string(),
        created_at: chrono::Utc::now(),
    };
    use_cases.repo.save_setup(&setup).unwrap();

    // Create a synthetic bundle zip:
    // Bundle/
    //   CodeMod/
    //     manifest.json (Name: "Bundle Code", UniqueID: "Author.BundleCode", EntryDll: "Bundle.dll")
    //     Bundle.dll
    //   PackMod/
    //     manifest.json (Name: "Bundle Pack", UniqueID: "Author.BundlePack", ContentPackFor: "Author.BundleCode")
    //     content.json
    let bundle_zip = root.join("TestBundle.zip");
    {
        let file = File::create(&bundle_zip).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        // Code Mod
        zip.start_file("TestBundle/CodeMod/manifest.json", options)
            .unwrap();
        let code_manifest = r#"{
            "Name": "Bundle Code",
            "Author": "Author",
            "Version": "1.0.0",
            "UniqueID": "Author.BundleCode",
            "EntryDll": "Bundle.dll"
        }"#;
        zip.write_all(code_manifest.as_bytes()).unwrap();

        zip.start_file("TestBundle/CodeMod/Bundle.dll", options)
            .unwrap();
        zip.write_all(b"fake dll content").unwrap();

        // Pack Mod (depends on CodeMod)
        zip.start_file("TestBundle/PackMod/manifest.json", options)
            .unwrap();
        let pack_manifest = r#"{
            "Name": "Bundle Pack",
            "Author": "Author",
            "Version": "1.0.0",
            "UniqueID": "Author.BundlePack",
            "ContentPackFor": {
                "UniqueID": "Author.BundleCode"
            }
        }"#;
        zip.write_all(pack_manifest.as_bytes()).unwrap();

        zip.start_file("TestBundle/PackMod/content.json", options)
            .unwrap();
        zip.write_all(b"{}").unwrap();

        zip.finish().unwrap();
    }

    let staging_dir = root.join("staging");
    let final_mods_dir = root.join("Mods");

    // Inspect
    let inspection =
        SafeZipExtractor::inspect_and_stage(&bundle_zip, &setup.id, &staging_dir, &use_cases.repo)
            .expect("Bundle inspection should succeed");

    assert_eq!(inspection.plan.component_manifests.len(), 2);
    assert!(inspection.plan.dependency_report.is_installable);

    // Commit Install
    let installed = use_cases
        .commit_mod_install(&inspection.plan, &staging_dir, &final_mods_dir)
        .unwrap();
    assert_eq!(installed.name, "Bundle Code");

    // Check DB: Both mods should be in installed_mods table
    let all_mods = use_cases.repo.list_installed_mods(&setup.id).unwrap();
    assert_eq!(all_mods.len(), 2);
    let uids: Vec<String> = all_mods.iter().map(|m| m.unique_id.clone()).collect();
    assert!(uids.contains(&"Author.BundleCode".to_string()));
    assert!(uids.contains(&"Author.BundlePack".to_string()));

    // Verify files on disk
    assert!(final_mods_dir
        .join(&inspection.plan.mod_folder_name)
        .join("CodeMod/Bundle.dll")
        .exists());
    assert!(final_mods_dir
        .join(&inspection.plan.mod_folder_name)
        .join("PackMod/content.json")
        .exists());

    // Remove bundle
    let recovery_dir = root.join("recovery");
    use_cases
        .remove_mod(&installed.id, &setup.id, &final_mods_dir, &recovery_dir)
        .unwrap();

    // Verify both removed from DB
    let remaining = use_cases.repo.list_installed_mods(&setup.id).unwrap();
    assert_eq!(remaining.len(), 0);
}

#[test]
#[ignore = "requires opt-in mod ZIP fixtures in SMM_MOD_FIXTURE_DIR"]
fn test_real_world_user_downloads_mods() {
    let home =
        std::env::var("SMM_MOD_FIXTURE_DIR").expect("set SMM_MOD_FIXTURE_DIR for opt-in tests");
    let ftm_zip = Path::new(&home).join("Farm Type Manager v1.26.1-3231-1-26-1-1763142930.zip");
    let sve_zip = Path::new(&home).join("-Stardew Valley Expanded--3753-1-15-11-1751325459.zip");

    assert!(
        ftm_zip.exists() && sve_zip.exists(),
        "Both documented mod fixtures must exist"
    );

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let setup_id = "test-real-world-setup";

    let game = GameInstallation {
        id: "game-real-world".to_string(),
        canonical_root: root.join("Game"),
        platform_kind: StoreKind::ManualFolder,
        detected_version: Some("1.6.14".to_string()),
        validated_at: chrono::Utc::now(),
        is_fresh: true,
        is_managed: true,
        validation_error: None,
    };
    repo.save_game(&game).unwrap();
    let setup = Setup {
        id: setup_id.to_string(),
        game_id: game.id.clone(),
        display_name: "Default".to_string(),
        relative_mods_dir: "Mods".to_string(),
        created_at: chrono::Utc::now(),
    };
    repo.save_setup(&setup).unwrap();

    // Pre-install Content Patcher so FTM's dependency is satisfied
    let cp_mod = InstalledMod {
        id: "mod-cp".to_string(),
        setup_id: setup_id.to_string(),
        package_id: "hash-cp".to_string(),
        unique_id: "Pathoschild.ContentPatcher".to_string(),
        name: "Content Patcher".to_string(),
        author: "Pathoschild".to_string(),
        version: "2.9.1".to_string(),
        description: None,
        raw_manifest: "{}".to_string(),
        relative_target_path: "ContentPatcher".to_string(),
        file_inventory: vec![],
        installed_at: chrono::Utc::now(),
    };
    repo.save_installed_mod(&cp_mod).unwrap();

    // 1. Inspect and stage Farm Type Manager (tests UTF-8 BOM handling)
    let staging_dir = root.join("staging");
    let ftm_inspection =
        SafeZipExtractor::inspect_and_stage(&ftm_zip, setup_id, &staging_dir, &repo)
            .expect("FTM inspection must succeed with BOM stripping");
    assert_eq!(
        ftm_inspection.plan.manifest.unique_id,
        "Esca.FarmTypeManager"
    );
    assert!(ftm_inspection.plan.dependency_report.is_installable);

    // Now save FTM as installed so SVE's dependencies are satisfied
    let ftm_mod = InstalledMod {
        id: "mod-ftm".to_string(),
        setup_id: setup_id.to_string(),
        package_id: "hash-ftm".to_string(),
        unique_id: "Esca.FarmTypeManager".to_string(),
        name: "Farm Type Manager".to_string(),
        author: "Esca".to_string(),
        version: "1.26.1".to_string(),
        description: None,
        raw_manifest: "{}".to_string(),
        relative_target_path: "FarmTypeManager".to_string(),
        file_inventory: vec![],
        installed_at: chrono::Utc::now(),
    };
    repo.save_installed_mod(&ftm_mod).unwrap();

    // 2. Inspect and stage Stardew Valley Expanded (tests multi-mod bundle handling)
    let sve_inspection =
        SafeZipExtractor::inspect_and_stage(&sve_zip, setup_id, &staging_dir, &repo)
            .expect("SVE inspection must succeed as multi-mod bundle");
    assert_eq!(sve_inspection.plan.component_manifests.len(), 3);
    assert_eq!(sve_inspection.plan.manifest.name, "Stardew Valley Expanded");
    assert!(sve_inspection.plan.dependency_report.is_installable);
}

#[test]
fn test_launch_session_polling_verification_and_termination() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let pkg_store = FilesystemPackageStore::new(root.join("packages"));
    let smapi_installer = ProcessSmapiInstaller::new(root.join("smapi_cache"));
    let launcher = DetachedGameLauncher::isolated();
    let mock_log_path = root.join("logs").join("SMAPI-latest.txt");
    let _ = std::fs::create_dir_all(mock_log_path.parent().unwrap());
    let log_reader = SmapiSessionLogReader::new(Some(mock_log_path.clone()));
    let lock = FileInstanceLock::new(root.join(".instance.lock"));

    let use_cases = CoreUseCases::new(repo, pkg_store, smapi_installer, launcher, log_reader, lock);

    // Create mock game folder and executable script
    let game_dir = root.join("Game");
    std::fs::create_dir_all(&game_dir).unwrap();
    let smapi_bin = game_dir.join("StardewModdingAPI");
    // Script sleeps so process stays alive
    std::fs::write(&smapi_bin, "#!/bin/sh\nsleep 30\n").unwrap();
    let mut perms = std::fs::metadata(&smapi_bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&smapi_bin, perms).unwrap();

    let game = GameInstallation {
        id: "game-launch-test".to_string(),
        canonical_root: game_dir.canonicalize().unwrap(),
        platform_kind: StoreKind::ManualFolder,
        detected_version: Some("1.6.14".to_string()),
        validated_at: chrono::Utc::now(),
        is_fresh: true,
        is_managed: true,
        validation_error: None,
    };
    use_cases.repo.save_game(&game).unwrap();

    let setup = Setup {
        id: "setup-launch-test".to_string(),
        game_id: game.id.clone(),
        display_name: "Default".to_string(),
        relative_mods_dir: "Mods".to_string(),
        created_at: chrono::Utc::now(),
    };
    use_cases.repo.save_setup(&setup).unwrap();

    let mod_item = InstalledMod {
        id: "mod-test".to_string(),
        setup_id: setup.id.clone(),
        package_id: "hash-test".to_string(),
        unique_id: "Author.TestMod".to_string(),
        name: "Test Mod".to_string(),
        author: "Author".to_string(),
        version: "1.0.0".to_string(),
        description: None,
        raw_manifest: "{}".to_string(),
        relative_target_path: "TestMod".to_string(),
        file_inventory: vec![],
        installed_at: chrono::Utc::now(),
    };
    use_cases.repo.save_installed_mod(&mod_item).unwrap();

    // 1. Launch the game
    let mods_dir = root.join("Mods");
    let session = use_cases
        .launch_game(&game.id, &setup.id, &mods_dir)
        .expect("Launch must succeed");
    assert_eq!(session.state, SessionState::RunningUnverified);
    assert!(session.pid.is_some());
    let pid = session.pid.unwrap();

    // 2. Poll session while unverified
    let polled = use_cases.poll_session(&session.id).unwrap().unwrap();
    assert_eq!(polled.state, SessionState::RunningUnverified);

    // 3. Write SMAPI log confirming the mod
    let now_str = chrono::Utc::now().to_rfc3339();
    let log_content = format!(
        "[12:00:00 INFO SMAPI] Mods go here: {}\n[12:00:00 INFO SMAPI] Log started at {} UTC\n[12:00:00 TRACE SMAPI] Test Mod (ID: Author.TestMod)\n[12:00:00 INFO SMAPI] Loaded 1 mods:\n[12:00:00 INFO SMAPI]    Test Mod 1.0.0 by Author | A test mod\n",
        mods_dir.display(), now_str
    );
    std::fs::write(&mock_log_path, &log_content).unwrap();

    // 4. Poll again: should transition to ModLoadConfirmed!
    let verified_session = use_cases.poll_session(&session.id).unwrap().unwrap();
    assert_eq!(verified_session.state, SessionState::ModLoadConfirmed);
    assert!(verified_session.verification_result.is_some());
    let res = verified_session.verification_result.unwrap();
    assert!(res.confirmed_mods.contains(&"Author.TestMod".to_string()));

    // 5. Check app snapshot returns active session
    let snap = use_cases.get_app_snapshot(Some(&game.id)).unwrap();
    assert!(snap.active_session.is_some());
    assert_eq!(
        snap.active_session.unwrap().state,
        SessionState::ModLoadConfirmed
    );

    // 6. Test SMAPI log retrieval
    let read_log = use_cases.get_smapi_log().unwrap();
    assert!(read_log.contains("Test Mod 1.0.0"));
    assert_eq!(use_cases.get_smapi_log_path(), mock_log_path);

    // 7. Terminate the game
    use_cases
        .terminate_game(Some(&session.id))
        .expect("Terminate must succeed");

    // 8. Poll should now report Exited
    let exited_session = use_cases.poll_session(&session.id).unwrap().unwrap();
    assert_eq!(exited_session.state, SessionState::Exited);
    assert!(!use_cases.launcher.is_game_running(Some(pid)));
}

#[test]
fn test_launch_session_detects_immediate_startup_crash() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let db_path = root.join("state.sqlite3");
    let repo = SqliteStateRepository::new(&db_path).unwrap();
    let pkg_store = FilesystemPackageStore::new(root.join("packages"));
    let smapi_installer = ProcessSmapiInstaller::new(root.join("smapi_cache"));
    let launcher = DetachedGameLauncher::isolated();
    let log_reader = SmapiSessionLogReader::new(None);
    let lock = FileInstanceLock::new(root.join(".instance.lock"));

    let use_cases = CoreUseCases::new(repo, pkg_store, smapi_installer, launcher, log_reader, lock);

    // Create mock game folder with executable script that immediately crashes
    let game_dir = root.join("Game");
    std::fs::create_dir_all(&game_dir).unwrap();
    let smapi_bin = game_dir.join("StardewModdingAPI");
    std::fs::write(&smapi_bin, "#!/bin/sh\nexit 1\n").unwrap();
    let mut perms = std::fs::metadata(&smapi_bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&smapi_bin, perms).unwrap();

    let game = GameInstallation {
        id: "game-crash-test".to_string(),
        canonical_root: game_dir.canonicalize().unwrap(),
        platform_kind: StoreKind::ManualFolder,
        detected_version: Some("1.6.14".to_string()),
        validated_at: chrono::Utc::now(),
        is_fresh: true,
        is_managed: true,
        validation_error: None,
    };
    use_cases.repo.save_game(&game).unwrap();

    let setup = Setup {
        id: "setup-crash-test".to_string(),
        game_id: game.id.clone(),
        display_name: "Default".to_string(),
        relative_mods_dir: "Mods".to_string(),
        created_at: chrono::Utc::now(),
    };
    use_cases.repo.save_setup(&setup).unwrap();

    let mods_dir = root.join("Mods");
    let session = use_cases
        .launch_game(&game.id, &setup.id, &mods_dir)
        .unwrap();
    assert_eq!(session.state, SessionState::RunningUnverified);

    // Give process a moment to exit
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Poll session: must detect premature termination and transition to Failed
    let polled = use_cases.poll_session(&session.id).unwrap().unwrap();
    assert_eq!(polled.state, SessionState::Failed);
    assert!(polled.verification_result.is_some());
    let res = polled.verification_result.unwrap();
    assert!(res.details.contains("terminated prematurely"));
}
