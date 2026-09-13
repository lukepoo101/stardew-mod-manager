use manager_core::install::{InstallPlan, InventoryEntryType};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub struct StagedContentVerifier;

impl StagedContentVerifier {
    pub fn verify_staged_content(plan: &InstallPlan, staged_dir: &Path) -> Result<(), String> {
        if std::fs::symlink_metadata(staged_dir)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("Staged root must not be a symlink".into());
        }
        if !staged_dir.exists() || !staged_dir.is_dir() {
            return Err(format!(
                "Staged mod directory '{}' does not exist or is not a directory",
                staged_dir.display()
            ));
        }

        if plan.trusted_inventory.is_empty() {
            return Err("Trusted inventory is empty; cannot verify staging integrity".to_string());
        }

        let mut expected_paths = HashSet::new();
        let mut expected_dirs = HashSet::new();

        for entry in &plan.trusted_inventory {
            let normalized_expected = entry.relative_path.replace('\\', "/");
            let path = staged_dir.join(&entry.relative_path);
            expected_paths.insert(normalized_expected.clone());

            let mut cur = std::path::Path::new(&normalized_expected).parent();
            while let Some(parent) = cur {
                let p_str = parent.to_string_lossy().replace('\\', "/");
                if !p_str.is_empty() && p_str != "." {
                    expected_dirs.insert(p_str);
                }
                cur = parent.parent();
            }

            let symlink_meta = std::fs::symlink_metadata(&path).map_err(|e| {
                format!(
                    "Missing expected entry '{}' in staging: {}",
                    entry.relative_path, e
                )
            })?;

            if symlink_meta.file_type().is_symlink() {
                return Err(format!(
                    "Security error: symlink found at staged path '{}'",
                    entry.relative_path
                ));
            }

            match entry.entry_type {
                InventoryEntryType::Directory => {
                    if !symlink_meta.is_dir() {
                        return Err(format!(
                            "Staged entry '{}' expected to be a directory but was not",
                            entry.relative_path
                        ));
                    }
                }
                InventoryEntryType::File => {
                    if !symlink_meta.is_file() {
                        return Err(format!(
                            "Staged entry '{}' expected to be a file but was not",
                            entry.relative_path
                        ));
                    }
                    if symlink_meta.len() != entry.size_bytes {
                        return Err(format!(
                            "Size mismatch for '{}': expected {} bytes, found {}",
                            entry.relative_path,
                            entry.size_bytes,
                            symlink_meta.len()
                        ));
                    }
                    if let Some(ref expected_hash) = entry.sha256_hash {
                        let mut file = File::open(&path).map_err(|e| {
                            format!(
                                "Failed to read staged file '{}': {}",
                                entry.relative_path, e
                            )
                        })?;
                        let mut hasher = Sha256::new();
                        let mut buf = [0u8; 65536];
                        loop {
                            let n = file.read(&mut buf).map_err(|e| e.to_string())?;
                            if n == 0 {
                                break;
                            }
                            hasher.update(&buf[..n]);
                        }
                        let actual_hash = format!("{:x}", hasher.finalize());
                        if !actual_hash.eq_ignore_ascii_case(expected_hash) {
                            return Err(format!(
                                "Hash mismatch for '{}': expected {}, found {}",
                                entry.relative_path, expected_hash, actual_hash
                            ));
                        }
                    }
                }
            }
        }

        // Walk staged_dir to ensure NO EXTRA files or directories exist
        fn collect_relative_paths(
            base: &Path,
            current: &Path,
            found: &mut Vec<String>,
        ) -> Result<(), String> {
            for entry in std::fs::read_dir(current).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                let symlink_meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                if symlink_meta.file_type().is_symlink() {
                    return Err(format!(
                        "Illegal symlink found in staged tree: {}",
                        path.display()
                    ));
                }
                let rel = path.strip_prefix(base).map_err(|e| e.to_string())?;
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                found.push(rel_str);
                if symlink_meta.is_dir() {
                    collect_relative_paths(base, &path, found)?;
                }
            }
            Ok(())
        }

        let mut actual_paths = Vec::new();
        collect_relative_paths(staged_dir, staged_dir, &mut actual_paths)?;

        for p in actual_paths {
            let target_path = staged_dir.join(&p);
            let is_file = target_path.is_file();
            if is_file {
                if !expected_paths.contains(&p) {
                    return Err(format!(
                        "Unexpected extraneous file found in staging: '{}'",
                        p
                    ));
                }
            } else {
                if !expected_paths.contains(&p) && !expected_dirs.contains(&p) {
                    return Err(format!(
                        "Unexpected extraneous directory found in staging: '{}'",
                        p
                    ));
                }
            }
        }

        Ok(())
    }
}

impl manager_app::ports::deployment::StagedContentVerifierPort for StagedContentVerifier {
    fn verify_staged(
        &self,
        plan: &InstallPlan,
        staged_dir: &Path,
    ) -> manager_app::error::AppResult<()> {
        Self::verify_staged_content(plan, staged_dir)
            .map_err(|e| manager_app::error::AppError::system("STAGED_VERIFICATION_FAILED", e))
    }
}
