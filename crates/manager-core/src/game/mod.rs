use crate::domain::{GameInstallation, StoreKind};
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};

pub const GAME_APP_ID: &str = "413150";

#[derive(Debug, Clone)]
pub struct GameValidationResult {
    pub is_valid: bool,
    pub is_fresh: bool,
    pub detected_version: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

pub fn extract_game_version(game_path: &Path) -> String {
    let deps_file = game_path.join("Stardew Valley.deps.json");
    if deps_file.is_file() {
        if let Ok(content) = fs::read_to_string(&deps_file) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                // Check targets
                if let Some(targets) = json.get("targets").and_then(|t| t.as_object()) {
                    for (_target_name, target_val) in targets {
                        if let Some(target_obj) = target_val.as_object() {
                            for key in target_obj.keys() {
                                if let Some(ver) = key.strip_prefix("Stardew Valley/") {
                                    return ver.to_string();
                                }
                            }
                        }
                    }
                }
                // Check libraries
                if let Some(libraries) = json.get("libraries").and_then(|l| l.as_object()) {
                    for key in libraries.keys() {
                        if let Some(ver) = key.strip_prefix("Stardew Valley/") {
                            return ver.to_string();
                        }
                    }
                }
            }
        }
    }
    "unknown".to_string()
}

fn validate_base_game(game_path: &Path) -> Result<String, (Option<String>, String)> {
    if !game_path.exists() || !game_path.is_dir() {
        return Err((
            Some("DIRECTORY_NOT_FOUND".to_string()),
            format!(
                "The folder '{}' does not exist or is not a directory",
                game_path.display()
            ),
        ));
    }

    // 1. Check for expected game markers
    let dll_marker = game_path.join("Stardew Valley.dll");
    let exe_marker = game_path.join("StardewValley");
    let legacy_dll = game_path.join("StardewValley.dll");

    let has_game_files = dll_marker.exists() || exe_marker.exists() || legacy_dll.exists();
    if !has_game_files {
        return Err((
            Some("GAME_MARKERS_NOT_FOUND".to_string()),
            "The selected folder does not contain Stardew Valley game files (missing Stardew Valley.dll or StardewValley executable)".to_string(),
        ));
    }

    // 2. Validate native Linux game identity (reject Proton / Windows-only installs)
    if exe_marker.exists() {
        if let Ok(bytes) = fs::read(&exe_marker) {
            let is_elf = bytes.starts_with(b"\x7fELF");
            let is_script = bytes.starts_with(b"#!");
            if !is_elf && !is_script && bytes.starts_with(b"MZ") {
                return Err((
                        Some("NOT_NATIVE_LINUX".to_string()),
                        "Proton/Windows installation detected. Only native Linux Stardew Valley installations are supported in this release.".to_string(),
                    ));
            }
        }
    } else if game_path.join("StardewValley.exe").exists() && !exe_marker.exists() {
        return Err((
            Some("NOT_NATIVE_LINUX".to_string()),
            "Proton/Windows installation detected. Please install the native Linux version of Stardew Valley via Steam.".to_string(),
        ));
    }

    // Check for Proton prefix directory inside game folder (e.g. pfx)
    if game_path.join("pfx").exists() {
        return Err((
            Some("NOT_NATIVE_LINUX".to_string()),
            "Proton prefix detected. Native Linux installation required.".to_string(),
        ));
    }

    // 3. Check write access using temporary probe file
    let probe_file = game_path.join(format!(".smm_probe_{}.tmp", uuid_v4_simple()));
    match fs::write(&probe_file, b"probe") {
        Ok(_) => {
            let _ = fs::remove_file(&probe_file);
        }
        Err(e) => {
            return Err((
                Some("DIRECTORY_NOT_WRITABLE".to_string()),
                format!("Cannot write to game folder (permission denied): {}", e),
            ));
        }
    }

    let version = extract_game_version(game_path);
    Ok(version)
}

pub fn validate_fresh_game(game_path: &Path, _platform_kind: StoreKind) -> GameValidationResult {
    let version = match validate_base_game(game_path) {
        Ok(v) => v,
        Err((code, msg)) => {
            return GameValidationResult {
                is_valid: false,
                is_fresh: false,
                detected_version: None,
                error_code: code,
                error_message: Some(msg),
            };
        }
    };

    // Freshness check: detect existing SMAPI or existing user mods
    let smapi_bin = game_path.join("StardewModdingAPI");
    let smapi_dll = game_path.join("StardewModdingAPI.dll");
    let smapi_internal = game_path.join("smapi-internal");

    let has_existing_smapi = smapi_bin.exists() || smapi_dll.exists() || smapi_internal.exists();
    if has_existing_smapi {
        return GameValidationResult {
            is_valid: true,
            is_fresh: false,
            detected_version: Some(version),
            error_code: Some("GAME_NOT_FRESH".to_string()),
            error_message: Some("SMAPI is already installed in this game directory. Migration of existing modding setups is not supported in the MVP.".to_string()),
        };
    }

    let mods_dir = game_path.join("Mods");
    if mods_dir.exists() && mods_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&mods_dir) {
            let non_empty = entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                !name.starts_with('.')
            });
            if non_empty {
                return GameValidationResult {
                    is_valid: true,
                    is_fresh: false,
                    detected_version: Some(version),
                    error_code: Some("GAME_NOT_FRESH".to_string()),
                    error_message: Some("Existing mods were detected in the game's Mods directory. The MVP only supports fresh, unmodded installations.".to_string()),
                };
            }
        }
    }

    GameValidationResult {
        is_valid: true,
        is_fresh: true,
        detected_version: Some(version),
        error_code: None,
        error_message: None,
    }
}

pub fn validate_managed_game(game_path: &Path, _platform_kind: StoreKind) -> GameValidationResult {
    let version = match validate_base_game(game_path) {
        Ok(v) => v,
        Err((code, msg)) => {
            return GameValidationResult {
                is_valid: false,
                is_fresh: false,
                detected_version: None,
                error_code: code,
                error_message: Some(msg),
            };
        }
    };

    let smapi_bin = game_path.join("StardewModdingAPI");
    let smapi_dll = game_path.join("StardewModdingAPI.dll");
    let smapi_internal = game_path.join("smapi-internal");

    let has_smapi = smapi_bin.exists() || smapi_dll.exists() || smapi_internal.exists();
    if !has_smapi {
        return GameValidationResult {
            is_valid: false,
            is_fresh: false,
            detected_version: Some(version),
            error_code: Some("SMAPI_NOT_FOUND".to_string()),
            error_message: Some("SMAPI executable not found in managed game directory".to_string()),
        };
    }

    GameValidationResult {
        is_valid: true,
        is_fresh: false,
        detected_version: Some(version),
        error_code: None,
        error_message: None,
    }
}

pub fn validate_game_directory(game_path: &Path, platform_kind: StoreKind) -> GameValidationResult {
    validate_fresh_game(game_path, platform_kind)
}

pub fn create_game_installation(
    id: &str,
    canonical_root: PathBuf,
    platform_kind: StoreKind,
) -> GameInstallation {
    let val = validate_fresh_game(&canonical_root, platform_kind);
    GameInstallation {
        id: id.to_string(),
        canonical_root,
        platform_kind,
        detected_version: val.detected_version,
        validated_at: Utc::now(),
        is_fresh: val.is_fresh,
        is_managed: false,
        validation_error: val.error_message,
    }
}

pub fn create_managed_game_installation(
    id: &str,
    canonical_root: PathBuf,
    platform_kind: StoreKind,
) -> GameInstallation {
    let val = validate_managed_game(&canonical_root, platform_kind);
    GameInstallation {
        id: id.to_string(),
        canonical_root,
        platform_kind,
        detected_version: val.detected_version,
        validated_at: Utc::now(),
        is_fresh: false,
        is_managed: val.is_valid,
        validation_error: val.error_message,
    }
}

fn uuid_v4_simple() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{}", duration.as_secs(), duration.subsec_nanos())
}
