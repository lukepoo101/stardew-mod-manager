//! Profile bundle archives: a recipe plus the packages it names.

#![allow(clippy::result_large_err)]

use manager_app::error::{AppError, AppResult};
use manager_app::ports::bundle::{
    BundleArchivePort, BundleContents, BundlePeek, BundledPackage, BundledSetting,
};
use manager_core::ids::{hash_to_hex, ArtifactHash};
use manager_core::install::MAX_COMPRESSED_BYTES;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const RECIPE_ENTRY: &str = "recipe.json";
const PACKAGE_PREFIX: &str = "packages/";
const SETTINGS_PREFIX: &str = "settings/";
const MAX_SETTING_BYTES: u64 = 1024 * 1024;
const MAX_SETTINGS_BYTES: u64 = 20 * 1024 * 1024;

/// `settings/<uniqueid>/<path>/config.json` -> (unique id, path), or why not.
fn setting_entry(name: &str) -> Result<(String, String), String> {
    let rest = name
        .strip_prefix(SETTINGS_PREFIX)
        .ok_or_else(|| format!("'{name}' is not a recognised bundle entry"))?;
    let (unique_id, relative) = rest
        .split_once('/')
        .ok_or_else(|| format!("'{name}' does not name a mod"))?;
    let plain =
        |part: &str| !part.is_empty() && part != "." && part != ".." && !part.contains('\\');
    if !plain(unique_id) || !relative.split('/').all(plain) {
        return Err(format!("'{name}' is not a plain path"));
    }
    if !relative
        .rsplit('/')
        .next()
        .is_some_and(|file| file.eq_ignore_ascii_case("config.json"))
    {
        return Err(format!("'{name}' is not a settings file"));
    }
    Ok((unique_id.to_string(), relative.to_string()))
}
const MAX_RECIPE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 5_001;
/// Across every package in one bundle.
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Default)]
pub struct ZipBundleArchive;

fn invalid(message: impl Into<String>) -> AppError {
    AppError::validation("INVALID_BUNDLE", message)
}

fn io_error(context: &str, error: impl std::fmt::Display) -> AppError {
    AppError::filesystem(context.to_string(), error.to_string())
}

/// `packages/<64 hex>.zip` -> the digest, or an explanation.
fn package_hash(name: &str) -> Result<ArtifactHash, String> {
    let file = name
        .strip_prefix(PACKAGE_PREFIX)
        .and_then(|rest| rest.strip_suffix(".zip"))
        .ok_or_else(|| format!("'{name}' is not a recognised bundle entry"))?;
    ArtifactHash::parse(file.to_lowercase())
        .map_err(|_| format!("'{name}' is not named after a SHA-256 digest"))
}

type Classified = (
    ZipArchive<File>,
    Vec<(usize, ArtifactHash)>,
    Vec<(usize, String, String)>,
);

/// Opens the archive and classifies every entry before anything is read.
fn open(path: &Path) -> AppResult<Classified> {
    let file = File::open(path).map_err(|e| io_error("Could not open the bundle", e))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| invalid(format!("This is not a readable bundle: {e}")))?;
    if archive.len() > MAX_ENTRIES {
        return Err(invalid("The bundle has too many entries"));
    }
    let mut seen = HashSet::new();
    let mut has_recipe = false;
    let mut packages = Vec::new();
    let mut settings = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|e| invalid(format!("A bundle entry is unreadable: {e}")))?;
        let name = entry.name().to_string();
        if !seen.insert(name.to_lowercase()) {
            return Err(invalid(format!("'{name}' appears more than once")));
        }
        if entry.is_dir() {
            return Err(invalid(format!(
                "'{name}' is a folder, which bundles do not use"
            )));
        }
        if name == RECIPE_ENTRY {
            has_recipe = true;
        } else if name.starts_with(SETTINGS_PREFIX) {
            let (unique_id, relative) = setting_entry(&name).map_err(invalid)?;
            settings.push((index, unique_id, relative));
        } else {
            packages.push((index, package_hash(&name).map_err(invalid)?));
        }
    }
    if !has_recipe {
        return Err(invalid("The bundle has no recipe.json"));
    }
    Ok((archive, packages, settings))
}

fn read_recipe(archive: &mut ZipArchive<File>) -> AppResult<String> {
    let entry = archive
        .by_name(RECIPE_ENTRY)
        .map_err(|e| invalid(format!("The recipe could not be read: {e}")))?;
    if entry.size() > MAX_RECIPE_BYTES {
        return Err(invalid("The recipe is too large"));
    }
    let mut text = String::new();
    entry
        .take(MAX_RECIPE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| invalid(format!("The recipe is not readable text: {e}")))?;
    if text.len() as u64 > MAX_RECIPE_BYTES {
        return Err(invalid("The recipe is too large"));
    }
    Ok(text)
}

impl BundleArchivePort for ZipBundleArchive {
    fn write_bundle(
        &self,
        dest_dir: &Path,
        base_name: &str,
        recipe_json: &str,
        packages: &[(ArtifactHash, PathBuf)],
        settings: &[BundledSetting],
    ) -> AppResult<PathBuf> {
        std::fs::create_dir_all(dest_dir)
            .map_err(|e| io_error("Could not create the destination folder", e))?;

        let mut target = dest_dir.join(format!("{base_name}.smm-bundle.zip"));
        let mut counter = 2;
        while target.exists() {
            target = dest_dir.join(format!("{base_name}-{counter}.smm-bundle.zip"));
            counter += 1;
        }
        let partial = target.with_extension("zip.part");

        let write = || -> AppResult<()> {
            let file =
                File::create(&partial).map_err(|e| io_error("Could not create the bundle", e))?;
            let mut zip = ZipWriter::new(file);
            zip.start_file(
                RECIPE_ENTRY,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .map_err(|e| io_error("Could not write the recipe", e))?;
            zip.write_all(recipe_json.as_bytes())
                .map_err(|e| io_error("Could not write the recipe", e))?;
            for (hash, source) in packages {
                // Packages are already zip archives, so storing them avoids
                // recompressing for nothing.
                zip.start_file(
                    format!("{PACKAGE_PREFIX}{}.zip", hash.as_str()),
                    SimpleFileOptions::default()
                        .compression_method(CompressionMethod::Stored)
                        .large_file(true),
                )
                .map_err(|e| io_error("Could not add a package", e))?;
                let mut input = File::open(source)
                    .map_err(|e| io_error("A stored package could not be read", e))?;
                std::io::copy(&mut input, &mut zip)
                    .map_err(|e| io_error("Could not write a package", e))?;
            }
            for setting in settings {
                let name = format!(
                    "{SETTINGS_PREFIX}{}/{}",
                    setting.unique_id.to_lowercase(),
                    setting.relative_path
                );
                setting_entry(&name).map_err(invalid)?;
                zip.start_file(
                    name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .map_err(|e| io_error("Could not add a settings file", e))?;
                zip.write_all(&setting.bytes)
                    .map_err(|e| io_error("Could not add a settings file", e))?;
            }
            let file = zip
                .finish()
                .map_err(|e| io_error("Could not finish the bundle", e))?;
            file.sync_all()
                .map_err(|e| io_error("Could not flush the bundle", e))
        };

        match write() {
            Ok(()) => {
                std::fs::rename(&partial, &target)
                    .map_err(|e| io_error("Could not finalise the bundle", e))?;
                Ok(target)
            }
            Err(error) => {
                let _ = std::fs::remove_file(&partial);
                Err(error)
            }
        }
    }

    fn peek_bundle(&self, path: &Path) -> AppResult<BundlePeek> {
        let (mut archive, packages, settings) = open(path)?;
        let recipe_json = read_recipe(&mut archive)?;
        let mut settings_for: Vec<String> = settings.into_iter().map(|(_, id, _)| id).collect();
        settings_for.sort();
        settings_for.dedup();
        Ok(BundlePeek {
            recipe_json,
            packages: packages.into_iter().map(|(_, hash)| hash).collect(),
            settings_for,
        })
    }

    fn discard_scratch(&self, dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    fn read_bundle(&self, path: &Path, extract_dir: &Path) -> AppResult<BundleContents> {
        let (mut archive, entries, setting_entries) = open(path)?;
        let recipe_json = read_recipe(&mut archive)?;
        std::fs::create_dir_all(extract_dir)
            .map_err(|e| io_error("Could not create the working folder", e))?;

        let mut packages = Vec::new();
        let mut total: u64 = 0;
        for (index, hash) in entries {
            let entry = archive
                .by_index(index)
                .map_err(|e| invalid(format!("A package could not be read: {e}")))?;
            if entry.size() > MAX_COMPRESSED_BYTES {
                return Err(invalid("A package in the bundle is too large"));
            }
            let target = extract_dir.join(format!("{}.zip", hash.as_str()));
            let mut out =
                File::create(&target).map_err(|e| io_error("Could not extract a package", e))?;
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 65536];
            let mut written: u64 = 0;
            let mut limited = entry.take(MAX_COMPRESSED_BYTES + 1);
            loop {
                let read = limited
                    .read(&mut buffer)
                    .map_err(|e| invalid(format!("A package could not be read: {e}")))?;
                if read == 0 {
                    break;
                }
                written += read as u64;
                total += read as u64;
                if written > MAX_COMPRESSED_BYTES || total > MAX_TOTAL_BYTES {
                    return Err(invalid("The bundle is larger than this manager accepts"));
                }
                hasher.update(&buffer[..read]);
                out.write_all(&buffer[..read])
                    .map_err(|e| io_error("Could not extract a package", e))?;
            }
            out.sync_all()
                .map_err(|e| io_error("Could not extract a package", e))?;
            if hash_to_hex(hasher.finalize()) != hash.as_str().to_lowercase() {
                let _ = std::fs::remove_file(&target);
                return Err(invalid(format!(
                    "A package does not match its checksum ({}); the bundle is damaged or has been altered",
                    hash.as_str()
                )));
            }
            packages.push(BundledPackage { hash, path: target });
        }
        let mut settings = Vec::new();
        let mut settings_total: u64 = 0;
        for (index, unique_id, relative_path) in setting_entries {
            let entry = archive
                .by_index(index)
                .map_err(|e| invalid(format!("A settings file could not be read: {e}")))?;
            let mut bytes = Vec::new();
            entry
                .take(MAX_SETTING_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| invalid(format!("A settings file could not be read: {e}")))?;
            settings_total += bytes.len() as u64;
            if bytes.len() as u64 > MAX_SETTING_BYTES || settings_total > MAX_SETTINGS_BYTES {
                return Err(invalid("The bundle's settings files are too large"));
            }
            settings.push(BundledSetting {
                unique_id,
                relative_path,
                bytes,
            });
        }
        Ok(BundleContents {
            recipe_json,
            packages,
            settings,
        })
    }
}

#[cfg(test)]
mod settings_entry_tests {
    use super::setting_entry;

    #[test]
    fn only_plain_config_json_paths_are_accepted() {
        assert_eq!(
            setting_entry("settings/a.mod/config.json").unwrap(),
            ("a.mod".to_string(), "config.json".to_string())
        );
        assert_eq!(
            setting_entry("settings/a.mod/assets/config.json")
                .unwrap()
                .1,
            "assets/config.json"
        );
        for bad in [
            "settings/a.mod/../config.json",
            "settings/../config.json",
            "settings/a.mod/manifest.json",
            "settings/a.mod/Mod.dll",
            "settings/a.mod\\config.json",
            "settings/config.json",
        ] {
            assert!(setting_entry(bad).is_err(), "{bad} should be refused");
        }
    }
}
