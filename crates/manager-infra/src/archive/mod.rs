use manager_core::dependency::{evaluate_bundle_dependencies, evaluate_dependencies};
use manager_core::ids::ModUniqueId;
use manager_core::install::*;
use manager_core::manifest::parse_manifest;
use manager_core::path_semantics::{PathSemantics, WindowsPathSemantics};

pub mod staged_verifier;
use sha2::{Digest, Sha256};
pub use staged_verifier::StagedContentVerifier;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use zip::ZipArchive;

struct StagingCleanup(Option<PathBuf>);
impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            // Staging cleanup has to survive the same transient locks the rest
            // of the filesystem layer does: an antivirus or indexer holding a
            // file open is exactly the moment this drop runs.
            let _ = crate::platform::shared::fs::remove_dir_all(path);
        }
    }
}

/// Extracts and validates mod archives.
///
/// Validation applies the strictest namespace the manager supports, on every
/// host. A package that cannot be extracted on Windows is rejected on Linux
/// too, because the same package is what the user will try to install on
/// Windows - and rejecting it before extraction is the only point at which the
/// decision is still the user's rather than the filesystem's.
pub struct SafeZipExtractor {
    semantics: &'static dyn PathSemantics,
}

/// The strictest namespace the manager supports, shared by every extractor.
static WINDOWS_SEMANTICS: WindowsPathSemantics = WindowsPathSemantics;

impl SafeZipExtractor {
    /// An extractor that always validates against the Windows file namespace.
    ///
    /// The host's own comparison rules are deliberately not used to select this.
    /// A mod package is distributed to every platform, so a name that a
    /// case-sensitive host would extract happily is the same name that fails,
    /// aliases another file, or names a device once the user moves it to
    /// Windows. The strict rules therefore apply everywhere rather than only on
    /// the host that happens to notice.
    pub fn new() -> Self {
        Self {
            semantics: &WINDOWS_SEMANTICS,
        }
    }

    /// An extractor with explicit path semantics.
    ///
    /// This is a test seam for describing what another host's rules would
    /// accept; the shipping constructor never selects anything but Windows.
    pub fn with_semantics(semantics: &'static dyn PathSemantics) -> Self {
        Self { semantics }
    }

    pub fn semantics(&self) -> &'static dyn PathSemantics {
        self.semantics
    }

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

        let hash = manager_core::ids::hash_to_hex(hasher.finalize());
        Ok((hash, total_bytes))
    }

    pub fn inspect_and_stage_with_deps(
        &self,
        zip_path: &Path,
        setup_id: &str,
        plan_id: &str,
        staging_dir: &Path,
        installed_tuples: &[(ModUniqueId, String)],
        smapi_version: Option<&str>,
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

        // Pass 1: security validation of every path, symlink and name collision.
        // Nothing is written to disk until the whole archive has been judged.
        let mut collision_keys: HashMap<String, String> = HashMap::new();
        for i in 0..entry_count {
            let entry = archive
                .by_index(i)
                .map_err(|e| format!("Corrupt zip entry at index {}: {}", i, e))?;

            let raw_name = entry
                .name()
                .map_err(|e| format!("Unreadable zip entry name at index {}: {}", i, e))?
                .to_string();
            let depth = raw_name
                .split(['/', '\\'])
                .filter(|part| !part.is_empty())
                .count();
            if depth > MAX_ENTRY_DEPTH {
                return Err(format!(
                    "Zip entry '{}' is nested {} folders deep, which exceeds the {} folder limit",
                    raw_name.chars().take(80).collect::<String>(),
                    depth,
                    MAX_ENTRY_DEPTH
                ));
            }
            if raw_name.len() > MAX_ENTRY_PATH_BYTES {
                return Err(format!(
                    "Zip entry name is {} bytes long, which exceeds the {} byte limit",
                    raw_name.len(),
                    MAX_ENTRY_PATH_BYTES
                ));
            }
            validate_entry_name(&raw_name)?;
            validate_entry_for_semantics(&raw_name, self.semantics)?;

            if let Some(mode) = entry.unix_mode() {
                if (mode & 0o170000) == 0o120000 {
                    return Err(format!(
                        "Zip entry '{}' is a symlink, which is not permitted",
                        raw_name
                    ));
                }
            }

            // Two entries that differ only in case are one file on Windows.
            // Extraction order would otherwise decide which one survives.
            if !entry.is_dir() {
                let key = self.semantics.entry_key(&raw_name);
                if let Some(existing) = collision_keys.get(&key) {
                    if existing != &raw_name {
                        return Err(format!(
                            "Archive contains two entries that are the same file on this host: '{}' and '{}'",
                            existing, raw_name
                        ));
                    }
                } else {
                    collision_keys.insert(key, raw_name.clone());
                }
            }

            total_uncompressed += entry.size();
            if total_uncompressed > MAX_UNCOMPRESSED_BYTES {
                return Err(format!(
                    "Archive exceeds maximum uncompressed size limit ({} bytes)",
                    MAX_UNCOMPRESSED_BYTES
                ));
            }

            check_expansion(total_uncompressed, compressed_size)?;

            let path = Path::new(&raw_name);
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
            return Err(NO_MANIFEST.to_string());
        }

        // Extraction writes every file before anything is published, so the
        // drive holding the staging area needs room for all of it.
        crate::free_space::ensure(
            staging_dir,
            total_uncompressed,
            "unpacking this mod",
            "the manager's data folder",
        )?;

        struct FoundManifest {
            manifest: manager_core::Manifest,
            raw_manifest: String,
            mod_root_prefix: PathBuf,
        }

        let mut found_manifests = Vec::new();
        for &idx in &manifest_indices {
            let mut manifest_entry = archive
                .by_index(idx)
                .map_err(|e| format!("Failed to read manifest entry: {}", e))?;
            let manifest_path_str = manifest_entry
                .name()
                .map_err(|e| format!("Failed to read manifest entry name: {}", e))?
                .to_string();
            let mut raw_bytes = Vec::new();
            manifest_entry
                .read_to_end(&mut raw_bytes)
                .map_err(|e| format!("Failed to read manifest.json content: {}", e))?;
            let raw_manifest_content = decode_manifest_text(&raw_bytes);

            let parsed_manifest = parse_manifest(&raw_manifest_content)
                .map_err(|e| format!("{MANIFEST_PROBLEM} '{manifest_path_str}': {e}"))?;
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

        // SMAPI treats a folder with a manifest as one mod and does not look
        // inside it for others, so a manifest inside another mod's folder
        // makes it unclear what the archive is. Say so rather than guess.
        for outer in &found_manifests {
            for inner in &found_manifests {
                if !std::ptr::eq(outer, inner)
                    && inner.mod_root_prefix != outer.mod_root_prefix
                    && inner.mod_root_prefix.starts_with(&outer.mod_root_prefix)
                {
                    let shown = |prefix: &Path| {
                        if prefix.as_os_str().is_empty() {
                            "the archive's top level".to_string()
                        } else {
                            format!("'{}'", prefix.display())
                        }
                    };
                    return Err(format!(
                        "This archive has a mod ({}) inside another mod's folder ({}), so it is unclear which to install. Check the download, or install the folder you want from an archive that holds only it.",
                        shown(&inner.mod_root_prefix),
                        shown(&outer.mod_root_prefix)
                    ));
                }
            }
        }
        if let Some(duplicate) = found_manifests.iter().enumerate().find_map(|(i, a)| {
            found_manifests[i + 1..]
                .iter()
                .any(|b| b.mod_root_prefix == a.mod_root_prefix)
                .then(|| a.mod_root_prefix.clone())
        }) {
            return Err(format!(
                "The folder '{}' holds more than one manifest.json, so it is unclear which describes the mod.",
                duplicate.display()
            ));
        }

        let original_filename = zip_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "mod.zip".to_string());

        let plan_id = plan_id.to_string();
        let selection_id = format!("sel-{}", manager_core::uuid_v4());

        let plan_staging_root = if staging_dir.ends_with(&plan_id) {
            staging_dir.to_path_buf()
        } else {
            staging_dir.join(&plan_id)
        };
        if plan_staging_root.exists() && !staging_dir.ends_with(&plan_id) {
            let _ = crate::platform::shared::fs::remove_dir_all(&plan_staging_root);
        }
        std::fs::create_dir_all(&plan_staging_root)
            .map_err(|e| format!("Failed to create plan staging folder: {}", e))?;

        let mut cleanup = StagingCleanup(Some(plan_staging_root.clone()));
        let plan = if found_manifests.len() == 1 {
            let single = found_manifests.remove(0);
            let folder_name = sanitize_folder_name(single.manifest.unique_id.as_str());
            let mod_staging_dir = plan_staging_root.join(&folder_name);
            std::fs::create_dir_all(&mod_staging_dir)
                .map_err(|e| format!("Failed to create mod staging folder: {}", e))?;

            let mut not_installed = Vec::new();
            let mut not_installed_sizes = Vec::new();
            let mut file_inventory = Vec::new();
            let mut trusted_inventory = Vec::new();
            let mut extracted_total: u64 = 0;

            for i in 0..entry_count {
                let mut entry = archive
                    .by_index(i)
                    .map_err(|e| format!("Failed to read entry {}: {}", i, e))?;
                let raw_name = entry
                    .name()
                    .map_err(|e| format!("Failed to read entry name {}: {}", i, e))?
                    .to_string();
                let entry_path = Path::new(&raw_name);

                let Ok(rel_path) = entry_path.strip_prefix(&single.mod_root_prefix) else {
                    if !raw_name.ends_with('/') {
                        not_installed.push(raw_name.clone());
                        not_installed_sizes.push((raw_name.clone(), entry.size()));
                    }
                    continue;
                };
                {
                    if rel_path.as_os_str().is_empty() {
                        continue;
                    }
                    let target_dest = mod_staging_dir.join(rel_path);
                    if !target_dest.starts_with(&mod_staging_dir) {
                        let _ = crate::platform::shared::fs::remove_dir_all(&plan_staging_root);
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
                            extracted_total += bytes_read as u64;
                            // The declared sizes were checked up front, but a
                            // header can lie, so what is actually written is
                            // bounded too.
                            if extracted_total > MAX_UNCOMPRESSED_BYTES {
                                let _ =
                                    crate::platform::shared::fs::remove_dir_all(&plan_staging_root);
                                return Err(format!(
                                    "Archive expanded beyond the {} byte limit while extracting '{}'",
                                    MAX_UNCOMPRESSED_BYTES, raw_name
                                ));
                            }
                            hasher.update(&buffer[..bytes_read]);
                            out_file.write_all(&buffer[..bytes_read]).map_err(|e| {
                                format!("Failed writing '{}': {}", target_dest.display(), e)
                            })?;
                        }
                        out_file.sync_all().map_err(|e| {
                            format!("Failed to flush '{}': {}", target_dest.display(), e)
                        })?;

                        let file_sha = manager_core::ids::hash_to_hex(hasher.finalize());
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
                    let _ = crate::platform::shared::fs::remove_dir_all(&plan_staging_root);
                    return Err(format!(
                        "Manifest declares EntryDll '{}', but that file was not found in the archive",
                        dll_name
                    ));
                }
            }

            let dep_report = evaluate_dependencies(
                &single.manifest,
                installed_tuples,
                smapi_version.or(Some(manager_core::smapi::PINNED_SMAPI_VERSION)),
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
                not_installed,
                not_installed_sizes,
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

            let mut not_installed = Vec::new();
            let mut not_installed_sizes = Vec::new();
            let mut file_inventory = Vec::new();
            let mut trusted_inventory = Vec::new();
            let mut extracted_total: u64 = 0;

            for i in 0..entry_count {
                let mut entry = archive
                    .by_index(i)
                    .map_err(|e| format!("Failed to read entry {}: {}", i, e))?;
                let raw_name = entry
                    .name()
                    .map_err(|e| format!("Failed to read entry name {}: {}", i, e))?
                    .to_string();
                let entry_path = Path::new(&raw_name);

                let rel_path = if !common_ancestor.as_os_str().is_empty() {
                    match entry_path.strip_prefix(&common_ancestor) {
                        Ok(p) => p,
                        Err(_) => {
                            if !raw_name.ends_with('/') {
                                not_installed.push(raw_name.clone());
                                not_installed_sizes.push((raw_name.clone(), entry.size()));
                            }
                            continue;
                        }
                    }
                } else {
                    entry_path
                };

                if rel_path.as_os_str().is_empty() {
                    continue;
                }

                let target_dest = mod_staging_dir.join(rel_path);
                if !target_dest.starts_with(&mod_staging_dir) {
                    let _ = crate::platform::shared::fs::remove_dir_all(&plan_staging_root);
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
                        extracted_total += bytes_read as u64;
                        if extracted_total > MAX_UNCOMPRESSED_BYTES {
                            let _ = crate::platform::shared::fs::remove_dir_all(&plan_staging_root);
                            return Err(format!(
                                "Archive expanded beyond the {} byte limit while extracting '{}'",
                                MAX_UNCOMPRESSED_BYTES, raw_name
                            ));
                        }
                        hasher.update(&buffer[..bytes_read]);
                        out_file.write_all(&buffer[..bytes_read]).map_err(|e| {
                            format!("Failed writing '{}': {}", target_dest.display(), e)
                        })?;
                    }
                    out_file.sync_all().map_err(|e| {
                        format!("Failed to flush '{}': {}", target_dest.display(), e)
                    })?;

                    let file_sha = manager_core::ids::hash_to_hex(hasher.finalize());
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
                        let _ = crate::platform::shared::fs::remove_dir_all(&plan_staging_root);
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

            let all_manifests: Vec<manager_core::Manifest> =
                found_manifests.iter().map(|f| f.manifest.clone()).collect();
            let dep_report = evaluate_bundle_dependencies(
                &all_manifests,
                installed_tuples,
                smapi_version.or(Some(manager_core::smapi::PINNED_SMAPI_VERSION)),
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
                not_installed,
                not_installed_sizes,
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

impl Default for SafeZipExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Rejects entry names that are never a legitimate relative path.
///
/// This is the platform-independent half of the check: absolute paths, drive
/// prefixes, traversal and non-normal components are refused everywhere.
pub fn validate_entry_name(name: &str) -> Result<(), String> {
    if name.contains('\0') {
        return Err("Zip entry contains null byte".to_string());
    }

    if name.starts_with('/') || name.starts_with('\\') {
        return Err(format!("Zip entry '{}' is an absolute path", name));
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
                if s == "." || s == ".." || s.chars().all(|ch| ch == '.') {
                    return Err(format!(
                        "Zip entry '{}' contains illegal directory component '{}'",
                        name, s
                    ));
                }
            }
            Component::CurDir => {}
        }
    }

    // Windows drive prefix check e.g. "C:" appears as a normal component on
    // POSIX, so it is rejected explicitly rather than by component kind.
    if name.len() >= 2 && name.chars().nth(1) == Some(':') {
        return Err(format!("Zip entry '{}' contains drive letter prefix", name));
    }

    Ok(())
}

/// Applies a filesystem's component rules to every component of an entry.
pub fn validate_entry_for_semantics(
    name: &str,
    semantics: &dyn PathSemantics,
) -> Result<(), String> {
    for component in Path::new(name).components() {
        let Component::Normal(raw) = component else {
            continue;
        };
        let rendered = raw.to_string_lossy();
        semantics
            .validate_component(&rendered)
            .map_err(|violation| {
                format!(
                    "Zip entry '{}' cannot be extracted on {}: {}",
                    name,
                    semantics.id(),
                    violation
                )
            })?;
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
    let mut trimmed = no_traversal
        .trim_matches(|c| c == '.' || c == '-' || c == '_' || c == ' ')
        .to_string();

    // A sanitized name still has to be usable on Windows: a UniqueID of "CON"
    // or a name ending in a space would be legal here and unextractable there.
    if trimmed.is_empty() {
        return "Mod".to_string();
    }
    if manager_core::path_semantics::WindowsPathSemantics::is_reserved_device_name(&trimmed) {
        trimmed.push_str("_mod");
    }
    if trimmed.ends_with('.') || trimmed.ends_with(' ') {
        trimmed.push('_');
    }
    if trimmed.is_empty() {
        "Mod".to_string()
    } else {
        trimmed
    }
}

impl manager_app::ports::deployment::ArchiveInspectorPort for SafeZipExtractor {
    fn inspect_and_stage(
        &self,
        zip_path: &Path,
        operation_id: &manager_core::ids::OperationId,
        staging_dir: &Path,
        installed_manifests: &[(manager_core::ids::ModUniqueId, String)],
        smapi_version: Option<&str>,
    ) -> manager_app::error::AppResult<InstallPlan> {
        let op_str = operation_id.to_string();
        self.inspect_and_stage_with_deps(
            zip_path,
            &op_str,
            &op_str,
            staging_dir,
            installed_manifests,
            smapi_version,
        )
        .map(|res| res.plan)
        .map_err(inspection_error)
    }
}

/// Reads a manifest.json the way SMAPI does (.NET `File.ReadAllText`): a
/// UTF-16 byte-order mark selects UTF-16, and otherwise the text is UTF-8
/// with any invalid byte replaced. An older mod saved in Latin-1 or
/// Windows-1252 therefore loads in SMAPI, with an odd character where an
/// accent was, and must not be refused here as a damaged download.
fn decode_manifest_text(bytes: &[u8]) -> String {
    let utf16 = |little: bool| {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                if little {
                    u16::from_le_bytes(*pair)
                } else {
                    u16::from_be_bytes(*pair)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, ..] => utf16(true),
        [0xFE, 0xFF, ..] => utf16(false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Prefix of an error about a manifest.json the archive holds.
const MANIFEST_PROBLEM: &str = "Problem in";
const NO_MANIFEST: &str = "No manifest.json found in the archive";

/// Turns an inspection failure into what the user can do about it.
///
/// Almost every failure is about the file the user chose: not a ZIP, no
/// manifest, a mistake in the mod's manifest, or a refused entry. Those are
/// the mod's problem, not the manager's, so they say what to do with the
/// download and are not sent to support. Only failures writing the manager's
/// own staging area are reported as the manager's.
pub fn inspection_error(message: String) -> manager_app::error::AppError {
    use manager_app::error::AppError;
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| message.starts_with(p));
    if starts(&[
        "Failed to create",
        "Failed writing",
        "Failed to flush",
        "Failed to open file for hashing",
        "Read error while hashing",
        // The archive opened here is the manager's own stored copy, already
        // hashed: failing to open it (a lock held by a scanner, permissions)
        // is not something a new download fixes.
        "Failed to open zip archive",
    ]) {
        return AppError::system("INSPECT_STAGE_FAILED", message);
    }
    if message.starts_with("Not enough free space") {
        return AppError::storage("NOT_ENOUGH_SPACE", message);
    }
    // Reading a manifest's bytes fails only when the entry's compressed data
    // is damaged; its text encoding never fails (see decode_manifest_text).
    let summary = if starts(&[
        "Failed to read zip archive",
        "Corrupt zip entry",
        "Unreadable zip entry",
        "Failed decompressing",
        "Failed to read entry",
        "Failed to read manifest",
    ]) {
        "This file is not a complete ZIP archive, so it cannot be installed. It may be damaged or only partly downloaded: download the mod again.".to_string()
    } else if message.starts_with(NO_MANIFEST) {
        "This archive has no manifest.json, so it is not a SMAPI mod as it is. Check that you downloaded the mod's main file, not an optional extra or a different format.".to_string()
    } else if starts(&[MANIFEST_PROBLEM, "Manifest declares"]) {
        format!("The mod has a mistake, so it cannot be installed. {message}. Download it again, or tell the mod's author.")
    } else if starts(&["Zip entry", "Archive "]) {
        format!("The archive was refused before anything was extracted. {message}.")
    } else {
        message.clone()
    };
    AppError::validation("ARCHIVE_NOT_INSTALLABLE", summary).with_details(message)
}
#[cfg(test)]
mod tests {
    use super::*;
    use manager_core::path_semantics::{PosixPathSemantics, WindowsPathSemantics};
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    static WINDOWS: WindowsPathSemantics = WindowsPathSemantics;
    static POSIX: PosixPathSemantics = PosixPathSemantics;

    fn archive_with(names: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mod.zip");
        let file = File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            br#"{"Name":"Test","Author":"Tester","Version":"1.0.0","UniqueID":"Tester.Test"}"#,
        )
        .unwrap();
        for name in names {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(b"fixture").unwrap();
        }
        zip.finish().unwrap();
        (tmp, path)
    }

    fn stage(semantics: &'static dyn PathSemantics, path: &Path, tmp: &Path) -> Result<(), String> {
        let extractor = SafeZipExtractor::with_semantics(semantics);
        extractor
            .inspect_and_stage_with_deps(path, "setup", "plan", &tmp.join("staging"), &[], None)
            .map(|_| ())
    }

    /// Stages with the shipping constructor, which must not vary by host.
    fn stage_default(path: &Path, tmp: &Path) -> Result<(), String> {
        SafeZipExtractor::new()
            .inspect_and_stage_with_deps(path, "setup", "plan", &tmp.join("staging"), &[], None)
            .map(|_| ())
    }

    #[test]
    fn names_windows_cannot_store_are_rejected_on_every_host() {
        for bad in [
            "Mod/CON.json",
            "Mod/aux.txt",
            "Mod/com1.dll",
            "Mod/config.json:stream",
            "Mod/name ",
            "Mod/name.",
            "Mod/que?stion.txt",
            "Mod/star*.txt",
            "Mod/pipe|name.txt",
        ] {
            let (tmp, path) = archive_with(&[bad]);
            let error = stage(&WINDOWS, &path, tmp.path()).unwrap_err();
            assert!(
                error.contains("cannot be extracted") || error.contains("not allowed"),
                "{} should be rejected, got: {}",
                bad,
                error
            );
        }
    }

    #[test]
    fn a_case_insensitive_host_rejects_colliding_entries_before_extraction() {
        let (tmp, path) = archive_with(&["Mod/Config.json", "Mod/config.json"]);
        let error = stage(&WINDOWS, &path, tmp.path()).unwrap_err();
        assert!(
            error.contains("same file on this host"),
            "unexpected error: {}",
            error
        );
        // Nothing may be left behind by a rejected archive.
        assert!(
            !tmp.path().join("staging").join("plan").exists(),
            "a rejected archive must not leave a staging tree"
        );
    }

    #[test]
    fn the_default_extractor_rejects_windows_illegal_names_on_every_host() {
        // The shipping constructor must not consult the host: the same package
        // is rejected on Linux, which is the only point at which the decision is
        // still the user's rather than the filesystem's.
        for bad in [
            "Mod/CON.txt",
            "Mod/CONIN$.txt",
            "Mod/aux.txt",
            "Mod/config.json:stream",
            "Mod/name.",
        ] {
            let (tmp, path) = archive_with(&[bad]);
            let error = stage_default(&path, tmp.path()).unwrap_err();
            assert!(
                error.contains("cannot be extracted") || error.contains("not allowed"),
                "{} should be rejected, got: {}",
                bad,
                error
            );
        }

        // Case collisions are one file on Windows, whichever host extracts them.
        let (tmp, path) = archive_with(&["Mod/file.json", "Mod/FILE.json"]);
        let error = stage_default(&path, tmp.path()).unwrap_err();
        assert!(
            error.contains("same file on this host"),
            "unexpected error: {}",
            error
        );
    }

    #[test]
    fn the_posix_semantics_are_only_a_test_seam() {
        // Explicit POSIX semantics remain reachable so a test can describe what
        // a case-sensitive host would store, but the shipping constructor never
        // selects them.
        let (tmp, path) = archive_with(&["Mod/Config.json", "Mod/config.json"]);
        stage(&POSIX, &path, tmp.path()).expect("POSIX can store both names");
        assert!(
            stage_default(&path, tmp.path()).is_err(),
            "the default extractor must enforce the Windows namespace on every host"
        );
    }

    #[test]
    fn traversal_and_absolute_entries_are_rejected_everywhere() {
        for bad in ["../escape.txt", "/etc/passwd", "Mod/../../escape.txt"] {
            let (tmp, path) = archive_with(&[bad]);
            for semantics in [&WINDOWS as &dyn PathSemantics, &POSIX] {
                assert!(
                    stage(semantics, &path, tmp.path()).is_err(),
                    "{} must be rejected by {}",
                    bad,
                    semantics.id()
                );
            }
        }
    }

    #[test]
    fn sanitized_folder_names_are_usable_on_windows() {
        assert_eq!(sanitize_folder_name("Tester.Console"), "Tester.Console");
        // A reserved device name would be unextractable on Windows.
        assert_ne!(sanitize_folder_name("CON"), "CON");
        assert!(!sanitize_folder_name("Mod.").ends_with('.'));
        assert!(!sanitize_folder_name("Mod ").ends_with(' '));
        assert_eq!(sanitize_folder_name("..."), "Mod");
    }

    fn archive_of(entries: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mod.zip");
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        for (name, body) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        (tmp, path)
    }

    fn manifest_json(id: &str) -> String {
        format!(r#"{{"Name":"{id}","Author":"A","Version":"1.0.0","UniqueID":"{id}"}}"#)
    }

    #[test]
    fn a_broken_download_is_explained_not_sent_to_support() {
        use manager_app::error::AppErrorCategory;
        let tmp = tempfile::tempdir().unwrap();
        let inspect = |path: &Path| {
            SafeZipExtractor::new()
                .inspect_and_stage_with_deps(path, "s", "p", &tmp.path().join("staging"), &[], None)
                .map(|_| ())
                .unwrap_err()
        };

        let not_zip = tmp.path().join("notazip.zip");
        std::fs::write(&not_zip, b"this is not a zip").unwrap();
        let error = inspection_error(inspect(&not_zip));
        assert_eq!(error.category, AppErrorCategory::Validation);
        assert!(
            error.summary.contains("not a complete ZIP"),
            "{}",
            error.summary
        );

        let (_keep, empty) = archive_of(&[("readme.txt", "hi")]);
        let error = inspection_error(inspect(&empty));
        assert_eq!(error.category, AppErrorCategory::Validation);
        assert!(
            error.summary.contains("no manifest.json"),
            "{}",
            error.summary
        );
        assert!(!error.summary.contains("?."), "{}", error.summary);

        let (_keep, bad_json) = archive_of(&[("Broken/manifest.json", "{\"Name\": ")]);
        let error = inspection_error(inspect(&bad_json));
        assert_eq!(error.category, AppErrorCategory::Validation);
        assert!(
            error.summary.contains("'Broken/manifest.json'"),
            "{}",
            error.summary
        );
        assert!(error.summary.contains("mod's author"), "{}", error.summary);

        let (_keep, no_id) = archive_of(&[(
            "NoId/manifest.json",
            r#"{"Name":"N","Author":"A","Version":"1.0.0"}"#,
        )]);
        let error = inspection_error(inspect(&no_id));
        assert!(error.summary.contains("'UniqueID'"), "{}", error.summary);

        let (_keep, unsafe_entry) = archive_with(&["../evil.txt"]);
        let error = inspection_error(inspect(&unsafe_entry));
        assert_eq!(error.category, AppErrorCategory::Validation);
        assert!(
            error.summary.starts_with("The archive was refused"),
            "{}",
            error.summary
        );

        // The manager's own staging failures are still the manager's.
        let own = inspection_error("Failed to create plan staging folder: denied".into());
        assert_ne!(own.category, AppErrorCategory::Validation);

        // So is failing to open its own stored copy: a new download would
        // not help.
        let error = inspection_error(inspect(&tmp.path().join("gone.zip")));
        assert_ne!(error.category, AppErrorCategory::Validation);
        assert!(!error.summary.contains("download"), "{}", error.summary);
        let locked = inspection_error("Failed to open zip archive 'packages/a.zip': denied".into());
        assert_ne!(locked.category, AppErrorCategory::Validation);
        assert!(!locked.summary.contains("download"), "{}", locked.summary);
    }

    fn archive_of_bytes(name: &str, body: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mod.zip");
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(body).unwrap();
        zip.finish().unwrap();
        (tmp, path)
    }

    #[test]
    fn a_manifest_saved_in_latin1_or_utf16_is_read_as_smapi_reads_it() {
        // "Author":"Zoë" with ë as the single Latin-1 byte 0xEB, as older
        // mods saved by Windows editors have it. SMAPI loads it, so the
        // manager must not call the download broken.
        let latin1 = b"{\"Name\":\"Old\",\"Author\":\"Zo\xEB\",\"Version\":\"1.0.0\",\"UniqueID\":\"Old.Mod\"}";
        let (tmp, path) = archive_of_bytes("Old/manifest.json", latin1);
        let plan = SafeZipExtractor::new()
            .inspect_and_stage_with_deps(&path, "s", "p", &tmp.path().join("staging"), &[], None)
            .expect("a Latin-1 manifest is readable")
            .plan;
        assert_eq!(plan.manifest.unique_id.as_str(), "Old.Mod");
        assert_eq!(plan.manifest.author, "Zo\u{FFFD}");

        let text = r#"{"Name":"Wide","Author":"Zoë","Version":"1.0.0","UniqueID":"Wide.Mod"}"#;
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        let (tmp, path) = archive_of_bytes("Wide/manifest.json", &utf16);
        let plan = SafeZipExtractor::new()
            .inspect_and_stage_with_deps(&path, "s", "p", &tmp.path().join("staging"), &[], None)
            .expect("a UTF-16 manifest with a byte-order mark is readable")
            .plan;
        assert_eq!(plan.manifest.author, "Zoë");
    }

    #[test]
    fn accented_and_non_latin_names_are_inspected_not_fatal() {
        // A four-byte stem with a two-byte character ("abé", "Zoë") used to
        // panic in the reserved-device check and abort the whole app.
        let manifest = manifest_json("Zoë.CaféMenu");
        let (tmp, path) = archive_of(&[
            ("Zoë.CaféMenu/manifest.json", &manifest),
            ("Zoë.CaféMenu/assets/abé.png", "x"),
            ("Zoë.CaféMenu/i18n/Bjö.json", "{}"),
            ("Zoë.CaféMenu/ÇOM1.txt", "x"),
            ("Zoë.CaféMenu/漢字/Noë.png", "x"),
        ]);
        let plan = SafeZipExtractor::new()
            .inspect_and_stage_with_deps(
                &path,
                "setup",
                "plan",
                &tmp.path().join("staging"),
                &[],
                None,
            )
            .expect("accented names are ordinary file names")
            .plan;
        assert_eq!(plan.manifest.unique_id.as_str(), "Zoë.CaféMenu");
        assert!(plan
            .file_inventory
            .iter()
            .any(|entry| entry.ends_with("abé.png")));

        // A real device name under an accented folder is still refused.
        let (tmp, path) = archive_of(&[
            ("Zoë/manifest.json", &manifest_json("Zoë.Mod")),
            ("Zoë/COM1.txt", "x"),
        ]);
        assert!(stage_default(&path, tmp.path())
            .unwrap_err()
            .contains("cannot be extracted"));
    }

    #[test]
    fn a_mod_inside_another_mods_folder_is_ambiguous() {
        let outer = manifest_json("A.Outer");
        let inner = manifest_json("A.Inner");
        let (tmp, path) = archive_of(&[
            ("Outer/manifest.json", &outer),
            ("Outer/Inner/manifest.json", &inner),
        ]);
        let error = stage_default(&path, tmp.path()).unwrap_err();
        assert!(error.contains("inside another mod's folder"), "{error}");
        assert!(
            error.contains("'Outer/Inner'") || error.contains("Outer\\Inner"),
            "{error}"
        );
    }

    #[test]
    fn mods_side_by_side_under_a_wrapper_are_fine() {
        let a = manifest_json("A.One");
        let b = manifest_json("A.Two");
        let (tmp, path) = archive_of(&[
            ("Wrapper/One/manifest.json", &a),
            ("Wrapper/Two/manifest.json", &b),
            ("Wrapper/README.txt", "hello"),
        ]);
        assert!(stage_default(&path, tmp.path()).is_ok());
    }

    #[test]
    fn files_outside_the_mod_folder_are_listed_not_installed() {
        let manifest = manifest_json("A.Mod");
        let (tmp, path) = archive_of(&[
            ("Wrapper/Mod/manifest.json", &manifest),
            ("Wrapper/Mod/Mod.dll", "dll"),
            ("Wrapper/README.txt", "read me"),
            ("Wrapper/screenshots/", ""),
        ]);
        let plan = SafeZipExtractor::new()
            .inspect_and_stage_with_deps(
                &path,
                "setup",
                "plan",
                &tmp.path().join("staging"),
                &[],
                None,
            )
            .unwrap()
            .plan;
        assert_eq!(plan.not_installed, vec!["Wrapper/README.txt".to_string()]);
        assert_eq!(
            plan.not_installed_sizes,
            vec![("Wrapper/README.txt".to_string(), 7)]
        );
    }

    #[test]
    fn entries_nested_too_deeply_are_refused() {
        let manifest = manifest_json("A.Deep");
        let deep = format!("{}file.txt", "d/".repeat(MAX_ENTRY_DEPTH));
        let (tmp, path) = archive_of(&[("Mod/manifest.json", &manifest), (&deep, "x")]);
        let error = stage_default(&path, tmp.path()).unwrap_err();
        assert!(error.contains("folder limit"), "{error}");
    }
}
