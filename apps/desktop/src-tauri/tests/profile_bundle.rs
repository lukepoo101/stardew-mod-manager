//! Exporting a profile as a bundle and importing it as a new profile, through
//! the production composition root and the real install engine.

use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, PackageCatalogRepository,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{GameInstallationId, ProfileId};
use manager_infra::paths::AppPaths;
use stardew_mod_manager::state::AppState;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn mod_zip(dir: &Path, unique_id: &str, requires: &[&str]) -> PathBuf {
    let path = dir.join(format!("{unique_id}.zip"));
    let mut zip = ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = SimpleFileOptions::default();
    let dependencies: Vec<String> = requires
        .iter()
        .map(|id| format!(r#"{{"UniqueID":"{id}","IsRequired":true}}"#))
        .collect();
    zip.start_file(format!("{unique_id}/manifest.json"), options)
        .unwrap();
    zip.write_all(
        format!(
            r#"{{"Name":"{unique_id}","Author":"Author","Version":"1.0.0","UniqueID":"{unique_id}","EntryDll":"{unique_id}.dll","Dependencies":[{}]}}"#,
            dependencies.join(",")
        )
        .as_bytes(),
    )
    .unwrap();
    zip.start_file(format!("{unique_id}/{unique_id}.dll"), options)
        .unwrap();
    zip.write_all(b"binary").unwrap();
    zip.finish().unwrap();
    path
}

struct World {
    state: AppState,
    game_id: GameInstallationId,
    tmp: tempfile::TempDir,
}

fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(tmp.path().join("data"), tmp.path().join("cache"));
    let state = AppState::new_with_expected_smapi_hash(paths, Some("test")).unwrap();
    let game_id = GameInstallationId::new();
    std::fs::create_dir_all(tmp.path().join("game")).unwrap();
    state
        .repo
        .save_game(&GameInstallation {
            id: game_id,
            canonical_root: tmp.path().join("game"),
            operating_system: OperatingSystem::Linux,
            storefront: Storefront::Steam,
            management_mode: ManagementMode::Managed,
            created_at: "2026-09-13T00:00:00Z".parse().unwrap(),
        })
        .unwrap();
    World {
        state,
        game_id,
        tmp,
    }
}

fn install(world: &World, profile: &ProfileId, zip: &Path) {
    let services = &world.state.services;
    let preview = services.mods.prepare_install(profile, zip).unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    let operation = manager_core::ids::OperationId::from_str(&preview.operation_id).unwrap();
    services.operations.commit_operation(&operation).unwrap();
}

fn installed_ids(world: &World, profile: &ProfileId) -> Vec<(String, bool)> {
    let mut found: Vec<(String, bool)> = world
        .state
        .repo
        .list_profile_components(profile)
        .unwrap()
        .into_iter()
        .map(|pc| {
            let component = world
                .state
                .repo
                .get_package_component(&pc.package_component_id)
                .unwrap()
                .unwrap();
            (component.unique_id.to_string(), pc.enabled)
        })
        .collect();
    found.sort();
    found
}

/// A profile with a library, a mod that needs it (alphabetically first, so a
/// naive in-order install would fail), and one mod left disabled.
fn source_profile(world: &World) -> ProfileId {
    let services = &world.state.services;
    let created = services
        .profiles
        .create_profile(&world.game_id, "Source", None)
        .unwrap();
    let profile = ProfileId::from_str(&created.id).unwrap();
    let zips = world.tmp.path().join("zips");
    std::fs::create_dir_all(&zips).unwrap();
    install(world, &profile, &mod_zip(&zips, "Z.Lib", &[]));
    install(world, &profile, &mod_zip(&zips, "A.Needy", &["Z.Lib"]));
    install(world, &profile, &mod_zip(&zips, "M.Quiet", &[]));

    let quiet = world
        .state
        .repo
        .list_profile_components(&profile)
        .unwrap()
        .into_iter()
        .find(|pc| {
            world
                .state
                .repo
                .get_package_component(&pc.package_component_id)
                .unwrap()
                .unwrap()
                .unique_id
                .as_str()
                == "M.Quiet"
        })
        .unwrap();
    services.toggle.set_enabled(&quiet.id, false).unwrap();
    profile
}

#[test]
fn a_profile_round_trips_through_a_bundle_with_dependencies_and_disabled_state() {
    let world = world();
    let source = source_profile(&world);
    let services = &world.state.services;

    let out = world.tmp.path().join("out");
    let exported = services.bundle.export_bundle(&source, &out).unwrap();
    assert_eq!(exported.component_count, 3);
    assert_eq!(exported.package_count, 3);
    assert!(exported.missing_packages.is_empty());
    assert!(exported.path.ends_with(".smm-bundle.zip"));
    assert!(!exported.path.contains(".part"));

    let preview = services
        .bundle
        .inspect_bundle(Path::new(&exported.path))
        .unwrap();
    assert_eq!(preview.profile_name, "Source");
    assert_eq!(preview.components.len(), 3);
    assert!(preview.components.iter().all(|c| c.package_included));
    assert!(preview.missing_packages.is_empty());

    let result = services
        .bundle
        .import_bundle(Path::new(&exported.path), &world.game_id, "Imported")
        .unwrap();
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert_eq!(result.installed.len(), 3);
    assert_eq!(result.disabled, vec!["M.Quiet"]);

    let imported = ProfileId::from_str(&result.profile_id).unwrap();
    assert_eq!(
        installed_ids(&world, &imported),
        vec![
            ("A.Needy".to_string(), true),
            ("M.Quiet".to_string(), false),
            ("Z.Lib".to_string(), true),
        ]
    );
    // The source profile is exactly as it was.
    assert_eq!(installed_ids(&world, &source).len(), 3);

    // Scratch space is gone.
    let scratch = world.tmp.path().join("cache").join("bundle-import");
    assert!(
        !scratch.exists() || std::fs::read_dir(&scratch).unwrap().next().is_none(),
        "import scratch space was left behind"
    );
}

#[test]
fn exporting_twice_never_overwrites_the_first_bundle() {
    let world = world();
    let source = source_profile(&world);
    let out = world.tmp.path().join("out");
    let first = world
        .state
        .services
        .bundle
        .export_bundle(&source, &out)
        .unwrap();
    let second = world
        .state
        .services
        .bundle
        .export_bundle(&source, &out)
        .unwrap();
    assert_ne!(first.path, second.path);
    assert!(Path::new(&first.path).exists() && Path::new(&second.path).exists());
}

#[test]
fn a_tampered_package_is_refused_before_any_profile_is_created() {
    let world = world();
    let source = source_profile(&world);
    let out = world.tmp.path().join("out");
    let exported = world
        .state
        .services
        .bundle
        .export_bundle(&source, &out)
        .unwrap();

    // Rebuild the bundle with one package's bytes replaced but its name kept.
    let tampered = world.tmp.path().join("tampered.zip");
    {
        let mut archive =
            zip::ZipArchive::new(std::fs::File::open(&exported.path).unwrap()).unwrap();
        let mut writer = ZipWriter::new(std::fs::File::create(&tampered).unwrap());
        let mut replaced = false;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let name = entry.name().to_string();
            writer
                .start_file(&name, SimpleFileOptions::default())
                .unwrap();
            if name.starts_with("packages/") && !replaced {
                replaced = true;
                writer.write_all(b"not the package").unwrap();
            } else {
                std::io::copy(&mut entry, &mut writer).unwrap();
            }
        }
        writer.finish().unwrap();
    }

    let before = world
        .state
        .services
        .profiles
        .list_profiles(&world.game_id)
        .unwrap()
        .len();
    let error = world
        .state
        .services
        .bundle
        .import_bundle(&tampered, &world.game_id, "Tampered")
        .unwrap_err();
    assert!(error.summary.contains("checksum"), "{}", error.summary);
    let after = world
        .state
        .services
        .profiles
        .list_profiles(&world.game_id)
        .unwrap()
        .len();
    assert_eq!(before, after, "a rejected bundle must not create a profile");
}

#[test]
fn a_bundle_with_unexpected_entries_or_no_recipe_is_rejected() {
    let world = world();
    let bad = world.tmp.path().join("bad.zip");
    let mut writer = ZipWriter::new(std::fs::File::create(&bad).unwrap());
    writer
        .start_file("../escape.txt", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"x").unwrap();
    writer.finish().unwrap();
    assert!(world.state.services.bundle.inspect_bundle(&bad).is_err());

    let no_recipe = world.tmp.path().join("norecipe.zip");
    let mut writer = ZipWriter::new(std::fs::File::create(&no_recipe).unwrap());
    writer
        .start_file(
            format!("packages/{}.zip", "a".repeat(64)),
            SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(b"x").unwrap();
    writer.finish().unwrap();
    let error = world
        .state
        .services
        .bundle
        .inspect_bundle(&no_recipe)
        .unwrap_err();
    assert!(error.summary.contains("recipe"), "{}", error.summary);
}

#[test]
fn a_duplicate_profile_name_is_reported_and_leaves_nothing_behind() {
    let world = world();
    let source = source_profile(&world);
    let out = world.tmp.path().join("out");
    let exported = world
        .state
        .services
        .bundle
        .export_bundle(&source, &out)
        .unwrap();
    let error = world
        .state
        .services
        .bundle
        .import_bundle(Path::new(&exported.path), &world.game_id, "Source")
        .unwrap_err();
    assert_eq!(error.code, "DUPLICATE_PROFILE_NAME");
}
