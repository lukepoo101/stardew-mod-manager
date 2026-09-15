use crate::archive::SafeZipExtractor;
use chrono::Utc;
use manager_core::domain::SmapiInstallationRecord;
use manager_core::ports::SmapiInstaller;
use manager_core::smapi::*;
use std::fs::File;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use zip::ZipArchive;

#[derive(Clone)]
pub struct ProcessSmapiInstaller {
    cache_dir: PathBuf,
    expected_sha256: String,
}

impl ProcessSmapiInstaller {
    pub fn new<P: AsRef<Path>>(cache_dir: P) -> Self {
        Self::new_with_expected_hash(cache_dir, PINNED_SMAPI_SHA256)
    }

    pub fn new_with_expected_hash<P: AsRef<Path>>(cache_dir: P, expected_sha256: &str) -> Self {
        Self {
            cache_dir: cache_dir.as_ref().to_path_buf(),
            expected_sha256: expected_sha256.to_string(),
        }
    }

    fn prepare_installer_bundle(&self, provided_archive: Option<&Path>) -> Result<PathBuf, String> {
        let zip_path = match provided_archive {
            Some(p) => p.to_path_buf(),
            None => {
                let policy = default_release_policy();
                let cached = self
                    .cache_dir
                    .join(format!("SMAPI-{}-installer.zip", policy.tested_version));
                let mut needs_download = true;

                if cached.exists() {
                    if let Ok((hash, _)) = SafeZipExtractor::compute_sha256(&cached) {
                        if hash.eq_ignore_ascii_case(&self.expected_sha256) {
                            needs_download = false;
                        } else {
                            // Corrupted cache: remove and retry
                            let _ = std::fs::remove_file(&cached);
                        }
                    } else {
                        let _ = std::fs::remove_file(&cached);
                    }
                }

                if needs_download {
                    let _ = std::fs::create_dir_all(&self.cache_dir);
                    let tmp_cached = self.cache_dir.join(format!(
                        "SMAPI-{}-installer-{}.tmp",
                        policy.tested_version,
                        manager_core::uuid_v4()
                    ));

                    let client = reqwest::blocking::Client::builder()
                        .user_agent("StardewModManager/0.1.0")
                        .build()
                        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

                    let mut resp = client
                        .get(
                            &policy
                                .platforms
                                .get(if cfg!(target_os = "windows") {
                                    "windows"
                                } else if cfg!(target_os = "macos") {
                                    "macos"
                                } else {
                                    "linux"
                                })
                                .ok_or_else(|| "SMAPI_PLATFORM_UNSUPPORTED".to_string())?
                                .url,
                        )
                        .send()
                        .map_err(|e| format!("SMAPI installer automatic download failed: {}", e))?;

                    if !resp.status().is_success() {
                        return Err(format!(
                            "SMAPI installer automatic download failed with HTTP status {}",
                            resp.status()
                        ));
                    }

                    let mut out_file = File::create(&tmp_cached)
                        .map_err(|e| format!("Could not write to cache directory: {}", e))?;
                    std::io::copy(&mut resp, &mut out_file)
                        .map_err(|e| format!("Failed to write downloaded archive: {}", e))?;
                    drop(out_file);

                    // Verify checksum before promoting .tmp file
                    let (computed_hash, _) = SafeZipExtractor::compute_sha256(&tmp_cached)?;
                    if !computed_hash.eq_ignore_ascii_case(&self.expected_sha256) {
                        let _ = std::fs::remove_file(&tmp_cached);
                        return Err(format!(
                            "SMAPI installer integrity verification failed! Expected SHA-256 '{}', but got '{}'",
                            self.expected_sha256, computed_hash
                        ));
                    }

                    std::fs::rename(&tmp_cached, &cached).map_err(|e| {
                        let _ = std::fs::remove_file(&tmp_cached);
                        format!("Failed to promote temporary download to cache: {}", e)
                    })?;
                }

                cached
            }
        };

        let (computed_hash, _) = SafeZipExtractor::compute_sha256(&zip_path)?;
        if !computed_hash.eq_ignore_ascii_case(&self.expected_sha256) {
            return Err(format!(
                "SMAPI installer integrity verification failed! Expected SHA-256 '{}', but got '{}'",
                self.expected_sha256, computed_hash
            ));
        }

        // Extract installer into cache
        let extracted_dir = self.cache_dir.join("extracted_installer");
        if extracted_dir.exists() {
            let _ = std::fs::remove_dir_all(&extracted_dir);
        }
        std::fs::create_dir_all(&extracted_dir)
            .map_err(|e| format!("Failed to create extracted installer directory: {}", e))?;

        let file = File::open(&zip_path)
            .map_err(|e| format!("Failed to open SMAPI installer archive: {}", e))?;
        let mut archive =
            ZipArchive::new(file).map_err(|e| format!("Corrupt SMAPI zip archive: {}", e))?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let outpath = match entry.enclosed_name() {
                Some(path) => extracted_dir.join(path),
                None => continue,
            };

            if entry.is_dir() {
                std::fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
            } else {
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                    }
                }

                let _ = std::fs::remove_file(&outpath);
                {
                    let mut outfile = File::create(&outpath).map_err(|e| e.to_string())?;
                    std::io::copy(&mut entry, &mut outfile).map_err(|e| e.to_string())?;
                    outfile.sync_all().map_err(|e| e.to_string())?;
                }

                #[cfg(unix)]
                if let Some(mode) = entry.unix_mode() {
                    let _ =
                        std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode));
                }
            }
        }

        let platform_key = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        let policy = default_release_policy();
        let installer_path = policy
            .platforms
            .get(platform_key)
            .ok_or_else(|| "SMAPI_PLATFORM_UNSUPPORTED".to_string())?
            .installer_path
            .clone();
        let candidate_exec = extracted_dir.join(installer_path);
        if candidate_exec.exists() {
            #[cfg(unix)]
            let _ =
                std::fs::set_permissions(&candidate_exec, std::fs::Permissions::from_mode(0o755));
            return Ok(candidate_exec);
        }

        Err(
            "SMAPI_ARCHIVE_LAYOUT_UNSUPPORTED: expected current-OS installer payload was not found"
                .to_string(),
        )
    }
}

/// Reads the SMAPI version recorded in the installation's `deps.json` so status
/// reporting reflects what is on disk rather than the pinned release.
pub fn detect_installed_smapi_version(game_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(game_dir.join("StardewModdingAPI.deps.json")).ok()?;
    let deps: serde_json::Value = serde_json::from_str(&text).ok()?;
    deps.get("targets")?
        .as_object()?
        .values()
        .filter_map(|target| target.as_object())
        .flat_map(|target| target.keys())
        .find_map(|key| key.strip_prefix("StardewModdingAPI/").map(str::to_owned))
}

fn platform_launcher_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "StardewModdingAPI.exe"
    } else {
        SMAPI_EXECUTABLE_NAME
    }
}

impl SmapiInstaller for ProcessSmapiInstaller {
    fn install_smapi(
        &self,
        game_path: &Path,
        installer_archive: Option<&Path>,
    ) -> Result<SmapiInstallationRecord, String> {
        let installer_bin = self.prepare_installer_bundle(installer_archive)?;

        let game_str = game_path.to_string_lossy().into_owned();

        let mut attempts = 0;
        let output = loop {
            match Command::new(&installer_bin)
                .current_dir(installer_bin.parent().unwrap_or_else(|| Path::new(".")))
                .args(["--install", "--game-path"])
                .arg(game_path)
                .arg("--no-prompt")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
            {
                Ok(out) => break out,
                Err(e) if e.raw_os_error() == Some(26) && attempts < 15 => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => return Err(format!("Failed to spawn SMAPI installer process: {}", e)),
            }
        };

        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);

        // Don't assume exit code zero proves installation succeeded!
        if !output.status.success() {
            return Err(format!(
                "SMAPI installer exited with error code {:?}\nStdout: {}\nStderr: {}",
                output.status.code(),
                stdout_str,
                stderr_str
            ));
        }

        if !stdout_str.contains("SMAPI is installed!") {
            return Err(format!(
                "SMAPI installer did not confirm successful installation.\nOutput: {}\nStderr: {}",
                stdout_str, stderr_str
            ));
        }

        // Revalidate required artifacts exist on disk
        let smapi_bin = game_path.join(platform_launcher_name());
        if !smapi_bin.exists() {
            return Err(format!(
                "SMAPI verification failed: Executable '{}' was not found after installation",
                smapi_bin.display()
            ));
        }

        let smapi_dll = game_path.join("StardewModdingAPI.dll");
        if !smapi_dll.exists() {
            return Err(format!(
                "SMAPI verification failed: DLL '{}' was not found after installation",
                smapi_dll.display()
            ));
        }

        let smapi_deps = game_path.join("StardewModdingAPI.deps.json");
        if !smapi_deps.exists() {
            return Err(format!(
                "SMAPI verification failed: Dependencies file '{}' was not found",
                smapi_deps.display()
            ));
        }

        let smapi_internal = game_path.join("smapi-internal");
        if !smapi_internal.exists() || !smapi_internal.is_dir() {
            return Err(format!(
                "SMAPI verification failed: 'smapi-internal' folder missing at '{}'",
                smapi_internal.display()
            ));
        }

        // Ensure executable permissions
        #[cfg(unix)]
        let _ = std::fs::set_permissions(&smapi_bin, std::fs::Permissions::from_mode(0o755));

        // Check bundled mods exist in game's Mods directory
        let mods_dir = game_path.join("Mods");
        let has_bundled =
            mods_dir.join("SaveBackup").exists() || mods_dir.join("ConsoleCommands").exists();
        if !has_bundled {
            tracing::warn!(
                "SMAPI bundled mods (SaveBackup/ConsoleCommands) not found in game/Mods"
            );
        }

        let rec = SmapiInstallationRecord {
            id: manager_core::uuid_v4(),
            game_id: game_str.to_string(), // Overridden by use_cases with relational game.id
            release_version: PINNED_SMAPI_VERSION.to_string(),
            adapter_version: "1.0.0".to_string(),
            observed_version: detect_installed_smapi_version(game_path),
            installed_at: Utc::now(),
        };

        Ok(rec)
    }
}

impl manager_app::ports::runtime::SmapiInspectorPort for ProcessSmapiInstaller {
    fn observe_smapi(
        &self,
        game_dir: &Path,
    ) -> manager_app::error::AppResult<manager_core::smapi::SmapiObservation> {
        let smapi_bin = game_dir.join(platform_launcher_name());
        let smapi_dll = game_dir.join("StardewModdingAPI.dll");
        let smapi_deps = game_dir.join("StardewModdingAPI.deps.json");
        let smapi_internal = game_dir.join("smapi-internal");

        let bin_exists = smapi_bin.exists();
        let is_installed = bin_exists || smapi_dll.exists() || smapi_internal.exists();
        let artifacts_valid = bin_exists
            && smapi_dll.exists()
            && smapi_deps.exists()
            && smapi_internal.exists()
            && smapi_internal.is_dir();

        let detected_version = if is_installed {
            detect_installed_smapi_version(game_dir)
        } else {
            None
        };
        let mut evidence = Vec::new();
        if is_installed {
            evidence.push("SMAPI installation files detected".to_string());
        }
        if artifacts_valid {
            evidence.push("All SMAPI artifacts verified on disk".to_string());
        }
        Ok(manager_core::smapi::SmapiObservation {
            is_present: is_installed,
            observed_version: detected_version,
            executable_present: bin_exists,
            evidence,
            observed_at: Utc::now(),
        })
    }
}

impl manager_app::ports::runtime::SmapiInstallerPort for ProcessSmapiInstaller {
    fn install_smapi(
        &self,
        game_id: &manager_core::ids::GameInstallationId,
        game_path: &Path,
        installer_archive: &Path,
    ) -> manager_app::error::AppResult<manager_core::smapi::ManagedSmapiInstallation> {
        manager_core::ports::SmapiInstaller::install_smapi(self, game_path, Some(installer_archive))
            .map(|_| manager_core::smapi::ManagedSmapiInstallation {
                game_installation_id: *game_id,
                release_version: PINNED_SMAPI_VERSION.to_string(),
                release_policy_id: "default".to_string(),
                installed_at: Utc::now(),
            })
            .map_err(|e| manager_app::error::AppError::system("SMAPI_INSTALL_FAILED", e))
    }
}
