use manager_core::install::*;
use manager_core::manifest::{evaluate_bundle_dependencies, evaluate_dependencies, parse_manifest};
use manager_core::ports::StateRepository;

pub mod staged_verifier;
use sha2::{Digest, Sha256};
pub use staged_verifier::StagedContentVerifier;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zip::ZipArchive;

type PendingPlans = HashMap<String, (InstallPlan, Instant, Option<PathBuf>)>;

#[derive(Clone)]
pub struct PendingInspectionStore {
    plans: Arc<Mutex<PendingPlans>>,
    ttl: Duration,
}

impl PendingInspectionStore {
    pub fn new() -> Self {
        Self::with_ttl(Duration::from_secs(1800)) // 30 minutes
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            plans: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    pub fn insert(&self, plan: InstallPlan) {
        self.insert_staged(plan, None);
    }

    pub fn insert_staged(&self, plan: InstallPlan, path: Option<PathBuf>) {
        let mut lock = self.plans.lock().unwrap();
        self.cleanup_expired_locked(&mut lock);
        lock.insert(plan.plan_id.clone(), (plan, Instant::now(), path));
    }

    pub fn take(&self, plan_id: &str) -> Option<InstallPlan> {
        let mut lock = self.plans.lock().unwrap();
        self.cleanup_expired_locked(&mut lock);
        lock.remove(plan_id).map(|(p, _, _)| p)
    }

    pub fn get(&self, plan_id: &str) -> Option<InstallPlan> {
        let mut lock = self.plans.lock().unwrap();
        self.cleanup_expired_locked(&mut lock);
        lock.get(plan_id).map(|(p, _, _)| p.clone())
    }

    pub fn remove(&self, plan_id: &str) {
        let mut lock = self.plans.lock().unwrap();
        if let Some((_, _, Some(path))) = lock.remove(plan_id) {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    fn cleanup_expired_locked(&self, lock: &mut PendingPlans) {
        let now = Instant::now();
        let ttl = self.ttl;
        lock.retain(|_, (_, time, path)| {
            if now.duration_since(*time) <= ttl {
                return true;
            }
            if let Some(path) = path {
                let _ = std::fs::remove_dir_all(path);
            }
            false
        });
    }
}

impl Default for PendingInspectionStore {
    fn default() -> Self {
        Self::new()
    }
}

struct StagingCleanup(Option<PathBuf>);
impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

pub struct SafeZipExtractor;

impl SafeZipExtractor {
    pub fn compute_sha256<P: AsRef<Path>>(path: P) -> Result<(String, u64), String> {
        let mut file = File::open(path.as_ref())
            .map_err(|e| format!("Failed to open file for hashing: {}", e))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut total_bytes = 0u64;

        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|e| format!("Read error while hashing: {}", e))?;
            if count == 0 {
                break;
            }
            total_bytes += count as u64;
            hasher.update(&buffer[..count]);
        }

        let hash = format!("{:x}", hasher.finalize());
        Ok((hash, total_bytes))
    }

    pub fn inspect_and_stage<R: StateRepository>(
        zip_path: &Path,
        setup_id: &str,
        staging_dir: &Path,
        repo: &R,
    ) -> Result<ArchiveInspectionResult, String> {
        let (hash, compressed_size) = Self::compute_sha256(zip_path)?;
        if compressed_size > MAX_COMPRESSED_BYTES {
            return Err(format!(
                "Archive exceeds maximum compressed size ({} bytes > {} bytes limit)",
                compressed_size, MAX_COMPRESSED_BYTES
            ));
        }

        let file = File::open(zip_path)
            .map_err(|e| format!("Failed to open zip archive '{}': {}", zip_path.display(), e))?;

        let mut archive =
            ZipArchive::new(file).map_err(|e| format!("Failed to read zip archive: {}", e))?;

        let entry_count = archive.len();
        if entry_count > MAX_ENTRY_COUNT {
            return Err(format!(
                "Archive exceeds maximum entry count ({} entries > {} limit)",
                entry_count, MAX_ENTRY_COUNT
            ));
        }

        let mut manifest_indices = Vec::new();
        let mut total_uncompressed: u64 = 0;

        // Pass 1: Security validation of all paths and symlinks
        for i in 0..entry_count {
            let entry = archive
                .by_index(i)
                .map_err(|e| format!("Corrupt zip entry at index {}: {}", i, e))?;

            let raw_name = entry.name();
            validate_entry_name(raw_name)?;

            if let Some(mode) = entry.unix_mode() {
                if (mode & 0o170000) == 0o120000 {
                    return Err(format!(
                        "Zip entry '{}' is a symlink, which is not permitted",
                        raw_name
                    ));
                }
            }

            total_uncompressed += entry.size();
            if total_uncompressed > MAX_UNCOMPRESSED_BYTES {
                return Err(format!(
                    "Archive exceeds maximum uncompressed size limit ({} bytes)",
                    MAX_UNCOMPRESSED_BYTES
                ));
            }

            let path = Path::new(raw_name);
            if let Some(file_name) = path.file_name() {
                if file_name
                    .to_string_lossy()
                    .eq_ignore_ascii_case("manifest.json")
                {
                    manifest_indices.push(i);
                }
            }
        }

        if manifest_indices.is_empty() {
            return Err(
                "No manifest.json found in archive. Is this a valid SMAPI mod?".to_string(),
            );
        }

        struct FoundManifest {
            manifest: manager_core::domain::Manifest,
            raw_manifest: String,
            mod_root_prefix: PathBuf,
        }

        let mut found_manifests = Vec::new();
        for &idx in &manifest_indices {
            let mut manifest_entry = archive
                .by_index(idx)
                .map_err(|e| format!("Failed to read manifest entry: {}", e))?;
            let manifest_path_str = manifest_entry.name().to_string();
            let mut raw_manifest_content = String::new();
            manifest_entry
                .read_to_string(&mut raw_manifest_content)
                .map_err(|e| format!("Failed to read manifest.json content: {}", e))?;

            let parsed_manifest = parse_manifest(&raw_manifest_content)?;
            let manifest_path = Path::new(&manifest_path_str);
            let mod_root_prefix = manifest_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf();

            found_manifests.push(FoundManifest {
                manifest: parsed_manifest,
                raw_manifest: raw_manifest_content,
                mod_root_prefix,
            });
        }

        let original_filename = zip_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "mod.zip".to_string());

        let plan_id = format!("plan-{}", manager_core::uuid_v4());
        let selection_id = format!("sel-{}", manager_core::uuid_v4());
        let existing_mods = repo.list_installed_mods(setup_id)?;

        let plan_staging_root = staging_dir.join(&plan_id);
        if plan_staging_root.exists() {
            let _ = std::fs::remove_dir_all(&plan_staging_root);
        }
        std::fs::create_dir_all(&plan_staging_root)
            .map_err(|e| format!("Failed to create plan staging folder: {}", e))?;

        let mut cleanup = StagingCleanup(Some(plan_staging_root.clone()));
        let plan = if found_manifests.len() == 1 {
            let single = found_manifests.remove(0);
            let folder_name = sanitize_folder_name(&single.manifest.unique_id);
            let mod_staging_dir = plan_staging_root.join(&folder_name);
            std::fs::create_dir_all(&mod_staging_dir)
                .map_err(|e| format!("Failed to create mod staging folder: {}", e))?;

            let mut file_inventory = Vec::new();
            let mut trusted_inventory = Vec::new();

            for i in 0..entry_count {
                let mut entry = archive
                    .by_index(i)
                    .map_err(|e| format!("Failed to read entry {}: {}", i, e))?;
                let raw_name = entry.name().to_string();
                let entry_path = Path::new(&raw_name);

                if let Ok(rel_path) = entry_path.strip_prefix(&single.mod_root_prefix) {
                    if rel_path.as_os_str().is_empty() {
                        continue;
                    }
                    let target_dest = mod_staging_dir.join(rel_path);
                    if !target_dest.starts_with(&mod_staging_dir) {
                        let _ = std::fs::remove_dir_all(&plan_staging_root);
                        return Err(format!(
                            "Extracted path '{}' escapes staging directory",
                            target_dest.display()
                        ));
                    }

                    let rel_normalized = rel_path.to_string_lossy().replace('\\', "/");

                    if entry.is_dir() {
                        std::fs::create_dir_all(&target_dest).map_err(|e| {
                            format!(
                                "Failed to create directory '{}': {}",
                                target_dest.display(),
                                e
                            )
                        })?;
                        trusted_inventory.push(InventoryEntry {
                            relative_path: rel_normalized,
                            entry_type: InventoryEntryType::Directory,
                            size_bytes: 0,
                            sha256_hash: None,
                        });
                    } else {
                        if let Some(parent) = target_dest.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| {
                                format!("Failed to create parent '{}': {}", parent.display(), e)
                            })?;
                        }
                        let mut out_file = File::create(&target_dest).map_err(|e| {
                            format!(
                                "Failed to create extracted file '{}': {}",
                                target_dest.display(),
                                e
                            )
                        })?;
                        let mut hasher = Sha256::new();
                        let mut buffer = [0u8; 65536];
                        let mut bytes_written = 0u64;

                        loop {
                            let bytes_read = entry.read(&mut buffer).map_err(|e| {
                                format!("Failed decompressing '{}': {}", raw_name, e)
                            })?;
                            if bytes_read == 0 {
                                break;
                            }
                            bytes_written += bytes_read as u64;
                            hasher.update(&buffer[..bytes_read]);
                            out_file.write_all(&buffer[..bytes_read]).map_err(|e| {
                                format!("Failed writing '{}': {}", target_dest.display(), e)
                            })?;
                        }

                        let file_sha = format!("{:x}", hasher.finalize());
                        file_inventory.push(rel_normalized.clone());
                        trusted_inventory.push(InventoryEntry {
                            relative_path: rel_normalized,
                            entry_type: InventoryEntryType::File,
                            size_bytes: bytes_written,
                            sha256_hash: Some(file_sha),
                        });
                    }
                }
            }

            if let Some(ref dll_name) = single.manifest.entry_dll {
                let dll_path = mod_staging_dir.join(dll_name);
                if !dll_path.exists() {
                    let _ = std::fs::remove_dir_all(&plan_staging_root);
                    return Err(format!(
                        "Manifest declares EntryDll '{}', but that file was not found in the archive",
                        dll_name
                    ));
                }
            }

            let dep_report = evaluate_dependencies(
                &single.manifest,
                &existing_mods,
                Some(manager_core::smapi::PINNED_SMAPI_VERSION),
            );

            InstallPlan {
                plan_id,
                setup_id: setup_id.to_string(),
                package_hash: hash.clone(),
                original_filename: original_filename.clone(),
                mod_folder_name: folder_name,
                manifest: single.manifest,
                raw_manifest: single.raw_manifest,
                file_inventory,
                trusted_inventory,
                dependency_report: dep_report,
                component_manifests: Vec::new(),
            }
        } else {
            // Multi-mod bundle
            let common_ancestor = {
                let prefixes: Vec<Vec<&std::ffi::OsStr>> = found_manifests
                    .iter()
                    .map(|f| {
                        f.mod_root_prefix
                            .components()
                            .map(|c| c.as_os_str())
                            .collect()
                    })
                    .collect();
                let mut common = PathBuf::new();
                if !prefixes.is_empty() {
                    let first = &prefixes[0];
                    for i in 0..first.len() {
                        let part = first[i];
                        if prefixes.iter().all(|p| i < p.len() && p[i] == part) {
                            common.push(part);
                        } else {
                            break;
                        }
                    }
                }
                common
            };

            let folder_name = if !common_ancestor.as_os_str().is_empty() {
                sanitize_folder_name(&common_ancestor.to_string_lossy())
            } else {
                let stem = zip_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "ModBundle".to_string());
                sanitize_folder_name(&stem)
            };

            let mod_staging_dir = plan_staging_root.join(&folder_name);
            std::fs::create_dir_all(&mod_staging_dir)
                .map_err(|e| format!("Failed to create bundle staging folder: {}", e))?;

            let mut file_inventory = Vec::new();
            let mut trusted_inventory = Vec::new();

            for i in 0..entry_count {
                let mut entry = archive
                    .by_index(i)
                    .map_err(|e| format!("Failed to read entry {}: {}", i, e))?;
                let raw_name = entry.name().to_string();
                let entry_path = Path::new(&raw_name);

                let rel_path = if !common_ancestor.as_os_str().is_empty() {
                    match entry_path.strip_prefix(&common_ancestor) {
                        Ok(p) => p,
                        Err(_) => continue,
                    }
                } else {
                    entry_path
                };

                if rel_path.as_os_str().is_empty() {
                    continue;
                }

                let target_dest = mod_staging_dir.join(rel_path);
                if !target_dest.starts_with(&mod_staging_dir) {
                    let _ = std::fs::remove_dir_all(&plan_staging_root);
                    return Err(format!(
                        "Extracted path '{}' escapes staging directory",
                        target_dest.display()
                    ));
                }

                let rel_normalized = rel_path.to_string_lossy().replace('\\', "/");

                if entry.is_dir() {
                    std::fs::create_dir_all(&target_dest).map_err(|e| {
                        format!(
                            "Failed to create directory '{}': {}",
                            target_dest.display(),
                            e
                        )
                    })?;
                    trusted_inventory.push(InventoryEntry {
                        relative_path: rel_normalized,
                        entry_type: InventoryEntryType::Directory,
                        size_bytes: 0,
                        sha256_hash: None,
                    });
                } else {
                    if let Some(parent) = target_dest.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            format!("Failed to create parent '{}': {}", parent.display(), e)
                        })?;
                    }
                    let mut out_file = File::create(&target_dest).map_err(|e| {
                        format!(
                            "Failed to create extracted file '{}': {}",
                            target_dest.display(),
                            e
                        )
                    })?;
                    let mut hasher = Sha256::new();
                    let mut buffer = [0u8; 65536];
                    let mut bytes_written = 0u64;

                    loop {
                        let bytes_read = entry
                            .read(&mut buffer)
                            .map_err(|e| format!("Failed decompressing '{}': {}", raw_name, e))?;
                        if bytes_read == 0 {
                            break;
                        }
                        bytes_written += bytes_read as u64;
                        hasher.update(&buffer[..bytes_read]);
                        out_file.write_all(&buffer[..bytes_read]).map_err(|e| {
                            format!("Failed writing '{}': {}", target_dest.display(), e)
                        })?;
                    }

                    let file_sha = format!("{:x}", hasher.finalize());
                    file_inventory.push(rel_normalized.clone());
                    trusted_inventory.push(InventoryEntry {
                        relative_path: rel_normalized,
                        entry_type: InventoryEntryType::File,
                        size_bytes: bytes_written,
                        sha256_hash: Some(file_sha),
                    });
                }
            }

            // Validate EntryDll for each sub-mod
            for fm in &found_manifests {
                if let Some(ref dll_name) = fm.manifest.entry_dll {
                    let sub_rel = if !common_ancestor.as_os_str().is_empty() {
                        fm.mod_root_prefix
                            .strip_prefix(&common_ancestor)
                            .unwrap_or(&fm.mod_root_prefix)
                    } else {
                        &fm.mod_root_prefix
                    };
                    let dll_path = mod_staging_dir.join(sub_rel).join(dll_name);
                    if !dll_path.exists() {
                        let _ = std::fs::remove_dir_all(&plan_staging_root);
                        return Err(format!(
                            "Manifest in '{}' declares EntryDll '{}', but that file was not found in the archive",
                            sub_rel.display(), dll_name
                        ));
                    }
                }
            }

            let mut component_manifests = Vec::new();
            for fm in &found_manifests {
                let sub_rel = if !common_ancestor.as_os_str().is_empty() {
                    fm.mod_root_prefix
                        .strip_prefix(&common_ancestor)
                        .unwrap_or(&fm.mod_root_prefix)
                        .to_string_lossy()
                        .to_string()
                } else {
                    fm.mod_root_prefix.to_string_lossy().to_string()
                };
                component_manifests.push(ComponentManifest {
                    manifest: fm.manifest.clone(),
                    raw_manifest: fm.raw_manifest.clone(),
                    relative_subfolder: sub_rel,
                });
            }

            let all_manifests: Vec<manager_core::domain::Manifest> =
                found_manifests.iter().map(|f| f.manifest.clone()).collect();
            let dep_report = evaluate_bundle_dependencies(
                &all_manifests,
                &existing_mods,
                Some(manager_core::smapi::PINNED_SMAPI_VERSION),
            );

            // Select primary manifest (prefer code mod with EntryDll, or first)
            let primary_idx = found_manifests
                .iter()
                .position(|f| f.manifest.entry_dll.is_some())
                .unwrap_or(0);
            let mut primary_manifest = found_manifests[primary_idx].manifest.clone();
            let primary_raw = found_manifests[primary_idx].raw_manifest.clone();

            let comp_names: Vec<String> = found_manifests
                .iter()
                .map(|f| f.manifest.name.clone())
                .collect();
            let bundle_note = format!(
                "Bundle includes {} components: {}.",
                found_manifests.len(),
                comp_names.join(", ")
            );
            primary_manifest.description = match primary_manifest.description {
                Some(d) => Some(format!("{}\n\n{}", bundle_note, d)),
                None => Some(bundle_note),
            };

            InstallPlan {
                plan_id,
                setup_id: setup_id.to_string(),
                package_hash: hash.clone(),
                original_filename: original_filename.clone(),
                mod_folder_name: folder_name,
                manifest: primary_manifest,
                raw_manifest: primary_raw,
                file_inventory,
                trusted_inventory,
                dependency_report: dep_report,
                component_manifests,
            }
        };

        cleanup.0 = None;
        Ok(ArchiveInspectionResult {
            selection_id,
            package_hash: hash,
            original_filename,
            byte_size: compressed_size,
            plan,
        })
    }
}

pub fn validate_entry_name(name: &str) -> Result<(), String> {
    if name.contains('\0') {
        return Err("Zip entry contains null byte".to_string());
    }

    if name.starts_with('/') || name.starts_with('\\') {
        return Err(format!("Zip entry '{}' is an absolute path", name));
    }

    // Windows drive prefix check e.g. "C:" or UNC "\\server"
    if name.len() >= 2 && name.chars().nth(1) == Some(':') {
        return Err(format!("Zip entry '{}' contains drive letter prefix", name));
    }

    let path = Path::new(name);
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(format!(
                    "Zip entry '{}' contains illegal parent traversal (..)",
                    name
                ));
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(format!("Zip entry '{}' contains illegal root prefix", name));
            }
            Component::Normal(c) => {
                let s = c.to_string_lossy();
                if s == "."
                    || s == ".."
                    || s.starts_with('.') && s.ends_with('.') && s.chars().all(|ch| ch == '.')
                {
                    return Err(format!(
                        "Zip entry '{}' contains illegal directory component '{}'",
                        name, s
                    ));
                }
            }
            Component::CurDir => {}
        }
    }

    Ok(())
}

pub fn sanitize_folder_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let no_traversal = sanitized.replace("..", "_");
    let trimmed = no_traversal.trim_matches(|c| c == '.' || c == '-' || c == '_' || c == ' ');
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "Mod".to_string()
    } else {
        trimmed.to_string()
    }
}
