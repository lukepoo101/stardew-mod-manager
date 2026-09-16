//! Failure-mode coverage for the infrastructure adapters that perform real
//! process, log and installer work.

#[cfg(target_os = "linux")]
use manager_app::ports::launcher::GameLauncherPort;
use manager_app::ports::logging::{ExpectedMod, SessionLogPort};
#[cfg(target_os = "linux")]
use manager_app::ports::runtime::SmapiInstallerPort;
#[cfg(target_os = "linux")]
use manager_core::ids::GameInstallationId;
use manager_core::ids::ModUniqueId;
#[cfg(target_os = "linux")]
use manager_core::smapi::PINNED_SMAPI_VERSION;
#[cfg(target_os = "linux")]
use manager_infra::launcher::DetachedGameLauncher;
use manager_infra::log_reader::SmapiSessionLogReader;
#[cfg(target_os = "linux")]
use manager_infra::smapi_adapter::ProcessSmapiInstaller;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use zip::write::SimpleFileOptions;
#[cfg(target_os = "linux")]
use zip::ZipWriter;

#[test]
fn log_errors_are_not_treated_as_load_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let log_path = tmp.path().join("SMAPI-latest.txt");

    // Capture the baseline while no log exists yet, then let SMAPI write a log
    // that starts this session but only reports a failed load.
    let reader = SmapiSessionLogReader::new(Some(log_path.clone()));
    let baseline = reader.capture_baseline().unwrap();
    std::fs::write(
        &log_path,
        format!(
            "[12:00:00 INFO  SMAPI] Log started at {}\n[12:00:01 ERROR SMAPI] A.Broken could not be loaded\n",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S UTC")
        ),
    )
    .unwrap();

    let result = reader
        .verify_session(
            &baseline,
            &[ExpectedMod {
                unique_id: ModUniqueId::new("A.Broken"),
                name: "Broken".to_string(),
                version: "1.0.0".to_string(),
            }],
        )
        .unwrap();
    assert!(result.session_matched);
    assert!(!result.all_mods_confirmed);
}

#[cfg(target_os = "linux")]
#[test]
fn an_untracked_process_is_never_signalled() {
    let mut child = std::process::Command::new("sleep")
        .arg("10")
        .spawn()
        .unwrap();
    let result = DetachedGameLauncher::isolated().terminate_game(Some(child.id()));
    let still_alive = child.try_wait().unwrap().is_none();
    child.kill().unwrap();
    child.wait().unwrap();

    assert!(result.is_err());
    assert!(still_alive);
}

#[cfg(target_os = "linux")]
fn synthetic_installer_zip(path: &Path, script: &str) -> String {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    zip.start_file(
        manager_core::smapi::PINNED_INSTALLER_INTERNAL_PATH,
        SimpleFileOptions::default().unix_permissions(0o755),
    )
    .unwrap();
    zip.write_all(script.as_bytes()).unwrap();
    zip.finish().unwrap();

    let (hash, _) = manager_infra::archive::SafeZipExtractor::compute_sha256(path).unwrap();
    hash
}

#[cfg(target_os = "linux")]
#[test]
fn an_installer_that_exits_with_an_error_code_fails_the_installation() {
    let tmp = tempfile::tempdir().unwrap();
    let game_dir = tmp.path().join("StardewValley");
    std::fs::create_dir_all(&game_dir).unwrap();

    let installer_zip = tmp.path().join("failing-installer.zip");
    let hash = synthetic_installer_zip(
        &installer_zip,
        r#"#!/bin/bash
echo "Fatal: could not patch game executable" >&2
exit 42
"#,
    );
    let installer = ProcessSmapiInstaller::new_with_expected_hash(tmp.path().join("cache"), &hash);

    let error = installer
        .install_smapi(&GameInstallationId::new(), &game_dir, &installer_zip)
        .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("42") || message.contains("Fatal: could not patch game executable"),
        "unexpected error: {message}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn an_installer_that_claims_success_without_artifacts_fails_verification() {
    let tmp = tempfile::tempdir().unwrap();
    let game_dir = tmp.path().join("StardewValley");
    std::fs::create_dir_all(&game_dir).unwrap();

    let installer_zip = tmp.path().join("incomplete-installer.zip");
    let hash = synthetic_installer_zip(
        &installer_zip,
        r#"#!/bin/bash
echo "SMAPI is installed!"
exit 0
"#,
    );
    let installer = ProcessSmapiInstaller::new_with_expected_hash(tmp.path().join("cache"), &hash);

    let error = installer
        .install_smapi(&GameInstallationId::new(), &game_dir, &installer_zip)
        .unwrap_err();
    assert!(
        error.to_string().contains("SMAPI verification failed"),
        "unexpected error: {error}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_successful_installer_records_the_pinned_release_version() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let game_dir = tmp.path().join("StardewValley");
    std::fs::create_dir_all(&game_dir).unwrap();

    let installer_zip = tmp.path().join("mock-smapi-installer.zip");
    let hash = synthetic_installer_zip(
        &installer_zip,
        r#"#!/bin/bash
while [ "$#" -gt 0 ]; do if [ "$1" = "--game-path" ]; then GAME_PATH="$2"; shift 2; else shift; fi; done
mkdir -p "$GAME_PATH/smapi-internal" "$GAME_PATH/Mods/SaveBackup"
printf '#!/bin/sh
' > "$GAME_PATH/StardewModdingAPI"
chmod +x "$GAME_PATH/StardewModdingAPI"
touch "$GAME_PATH/StardewModdingAPI.dll"
printf '{"targets":{"net6.0/linux-x64":{"StardewModdingAPI/4.1.10":{}}}}' > "$GAME_PATH/StardewModdingAPI.deps.json"
echo "SMAPI is installed!"
"#,
    );
    let installer = ProcessSmapiInstaller::new_with_expected_hash(tmp.path().join("cache"), &hash);

    let game_id = GameInstallationId::new();
    let record = installer
        .install_smapi(&game_id, &game_dir, &installer_zip)
        .unwrap();
    assert_eq!(record.game_installation_id, game_id);
    assert_eq!(record.release_version, PINNED_SMAPI_VERSION);

    // The installer leaves files behind with the mode the adapter must restore.
    let executable = game_dir.join(manager_core::smapi::SMAPI_EXECUTABLE_NAME);
    let mode = std::fs::metadata(&executable).unwrap().permissions().mode();
    assert_ne!(mode & 0o111, 0);
}
