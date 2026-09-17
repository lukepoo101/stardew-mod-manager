//! Archive inspection and staged-content integrity coverage.
//!
//! These assertions protect the security and integrity contract of the archive
//! adapter that every mod installation path depends on.

use manager_app::ports::deployment::{ArchiveInspectorPort, StagedContentVerifierPort};
use manager_core::ids::OperationId;
use manager_core::install::InstallPlan;
use manager_infra::archive::{SafeZipExtractor, StagedContentVerifier};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn create_synthetic_mod_zip(path: &Path, manifest_json: &str, extra_files: &[(&str, &[u8])]) {
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

fn inspect(zip_path: &Path, staging_dir: &Path) -> Result<InstallPlan, String> {
    SafeZipExtractor::new()
        .inspect_and_stage(
            zip_path,
            &OperationId::new(),
            staging_dir,
            &[],
            Some(manager_core::smapi::PINNED_SMAPI_VERSION),
        )
        .map_err(|error| error.to_string())
}

#[test]
fn adversarial_zip_traversal_is_rejected() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();

    let bad_zip = root.join("traversal.zip");
    let file = File::create(&bad_zip).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    zip.start_file("manifest.json", options).unwrap();
    zip.write_all(br#"{"Name":"Evil","Author":"Bad","Version":"1.0","UniqueID":"Evil.Mod","EntryDll":"bad.dll"}"#).unwrap();
    zip.start_file("../../../etc/shadow", options).unwrap();
    zip.write_all(b"attack payload").unwrap();
    zip.finish().unwrap();

    let error = inspect(&bad_zip, &root.join("staging")).unwrap_err();
    assert!(
        error.contains("illegal parent traversal"),
        "unexpected error: {error}"
    );
}

#[test]
fn a_manifest_declaring_a_missing_entry_dll_is_rejected() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let zip_path = root.join("missing_dll.zip");
    create_synthetic_mod_zip(
        &zip_path,
        r#"{
            "Name": "Missing Dll Mod",
            "Author": "Tester",
            "Version": "1.0.0",
            "UniqueID": "Tester.MissingDll",
            "EntryDll": "NonExistent.dll"
        }"#,
        &[],
    );

    let error = inspect(&zip_path, &root.join("staging")).unwrap_err();
    assert!(
        error.contains("declares EntryDll 'NonExistent.dll', but that file was not found"),
        "unexpected error: {error}"
    );
}

#[test]
fn a_missing_required_dependency_is_reported_on_the_inspection_plan() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let zip_path = root.join("dep_mod.zip");
    create_synthetic_mod_zip(
        &zip_path,
        r#"{
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
        }"#,
        &[("Dep.dll", b"binary")],
    );

    let plan = inspect(&zip_path, &root.join("staging")).unwrap();
    assert!(!plan.dependency_report.is_installable);
    assert_eq!(plan.dependency_report.findings.len(), 1);
    assert!(!plan.dependency_report.findings[0].satisfied);
}

#[test]
fn a_multi_component_bundle_is_inspected_as_one_plan_with_component_manifests() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let bundle_zip = root.join("TestBundle.zip");
    {
        let file = File::create(&bundle_zip).unwrap();
        let mut zip = ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        zip.start_file("TestBundle/CodeMod/manifest.json", options)
            .unwrap();
        zip.write_all(
            br#"{"Name":"Bundle Code","Author":"Author","Version":"1.0.0","UniqueID":"Author.BundleCode","EntryDll":"Bundle.dll"}"#,
        )
        .unwrap();
        zip.start_file("TestBundle/CodeMod/Bundle.dll", options)
            .unwrap();
        zip.write_all(b"fake dll content").unwrap();
        zip.start_file("TestBundle/PackMod/manifest.json", options)
            .unwrap();
        zip.write_all(
            br#"{"Name":"Bundle Pack","Author":"Author","Version":"1.0.0","UniqueID":"Author.BundlePack","ContentPackFor":{"UniqueID":"Author.BundleCode"}}"#,
        )
        .unwrap();
        zip.start_file("TestBundle/PackMod/content.json", options)
            .unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();
    }

    let staging_dir = root.join("staging");
    let plan = inspect(&bundle_zip, &staging_dir).unwrap();

    assert_eq!(plan.component_manifests.len(), 2);
    assert!(plan.dependency_report.is_installable);
    let unique_ids: Vec<String> = plan
        .component_manifests
        .iter()
        .map(|component| component.manifest.unique_id.to_string())
        .collect();
    assert!(unique_ids.contains(&"Author.BundleCode".to_string()));
    assert!(unique_ids.contains(&"Author.BundlePack".to_string()));
}

#[test]
fn tampered_staged_content_fails_verification() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let zip_path = root.join("mod.zip");
    create_synthetic_mod_zip(
        &zip_path,
        r#"{"Name":"Test","Author":"A","Version":"1.0.0","UniqueID":"A.Test","EntryDll":"test.dll"}"#,
        &[("test.dll", b"original")],
    );

    let operation_id = OperationId::new();
    let staging_dir = root.join("staging");
    let plan = SafeZipExtractor::new()
        .inspect_and_stage(
            &zip_path,
            &operation_id,
            &staging_dir,
            &[],
            Some(manager_core::smapi::PINNED_SMAPI_VERSION),
        )
        .unwrap();

    let staged = staging_dir
        .join(operation_id.to_string())
        .join(&plan.mod_folder_name);
    StagedContentVerifier
        .verify_staged(&plan, &staged)
        .expect("untampered staging verifies");

    std::fs::write(staged.join("test.dll"), b"tampered").unwrap();
    let error = StagedContentVerifier
        .verify_staged(&plan, &staged)
        .unwrap_err();
    assert!(
        error.to_string().contains("Hash mismatch"),
        "unexpected error: {error}"
    );
}

#[test]
#[ignore = "requires opt-in mod ZIP fixtures in SMM_MOD_FIXTURE_DIR"]
fn real_world_user_downloads_are_inspected() {
    let home =
        std::env::var("SMM_MOD_FIXTURE_DIR").expect("set SMM_MOD_FIXTURE_DIR for opt-in tests");
    let ftm_zip = Path::new(&home).join("Farm Type Manager v1.26.1-3231-1-26-1-1763142930.zip");
    let sve_zip = Path::new(&home).join("-Stardew Valley Expanded--3753-1-15-11-1751325459.zip");

    assert!(
        ftm_zip.exists() && sve_zip.exists(),
        "Both documented mod fixtures must exist"
    );

    let tmp = tempdir().unwrap();
    let staging_dir = tmp.path().join("staging");

    // Farm Type Manager exercises UTF-8 BOM handling in manifest parsing.
    let ftm_plan = inspect(&ftm_zip, &staging_dir).expect("FTM inspection must succeed");
    assert_eq!(ftm_plan.manifest.unique_id.as_str(), "Esca.FarmTypeManager");

    // Stardew Valley Expanded exercises multi-component bundle handling.
    let sve_plan = inspect(&sve_zip, &staging_dir).expect("SVE inspection must succeed");
    assert_eq!(sve_plan.component_manifests.len(), 3);
    assert_eq!(sve_plan.manifest.name, "Stardew Valley Expanded");
}
