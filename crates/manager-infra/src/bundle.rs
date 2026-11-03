//! Profile bundle archives: a recipe plus the packages it names.

#![allow(clippy::result_large_err)]

use manager_app::error::{AppError, AppResult};
use manager_app::ports::bundle::{
    BundleArchivePort, BundleContents, BundlePeek, BundledPackage, BundledSetting, PackageManifest,
};
use manager_core::ids::{hash_to_hex, ArtifactHash};
use manager_core::install::{MAX_COMPRESSED_BYTES, MAX_ENTRY_COUNT};
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
/// The documented limit for one settings file, as in a recipe.
const MAX_SETTING_BYTES: u64 = manager_core::recipe::MAX_SETTING_BYTES as u64;
const MAX_SETTINGS_BYTES: u64 = 20 * 1024 * 1024;
/// A manifest larger than this is not a manifest.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// More mods than this in one package is not a mod package.
const MAX_MANIFESTS: usize = 200;

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
    String,
);

/// Reads a whole file through SHA-256, writing what it read to `copy` too
/// when one is given.
fn hash_file(file: &mut File, mut copy: Option<&mut File>) -> AppResult<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| io_error("Could not read the bundle", e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        if let Some(out) = copy.as_deref_mut() {
            out.write_all(&buffer[..read])
                .map_err(|e| io_error("Could not copy the bundle", e))?;
        }
    }
    Ok(hash_to_hex(hasher.finalize()))
}

/// Opens the archive, works out the SHA-256 of the file and classifies every
/// entry before anything is read. The digest and the entries come from the
/// same open file, so a file replaced meanwhile cannot mix the two.
fn open(path: &Path) -> AppResult<Classified> {
    let mut file = File::open(path).map_err(|e| io_error("Could not open the bundle", e))?;
    let digest = hash_file(&mut file, None)?;
    std::io::Seek::rewind(&mut file).map_err(|e| io_error("Could not read the bundle", e))?;
    classify(file, digest)
}

/// Classifies every entry of an opened bundle before anything is read.
fn classify(file: File, digest: String) -> AppResult<Classified> {
    let mut archive = ZipArchive::new(file)
        .map_err(|e| invalid(format!("This is not a readable bundle: {e}")))?;
    // Bundles are written from their first byte. A zip found further in is
    // a package inside a bundle whose end is missing, usually one cut short
    // while it was copied.
    if archive.offset() > 0 {
        return Err(invalid(
            "This bundle is incomplete: its end is missing, so it was probably cut short while it was copied or downloaded. Ask for it again.",
        ));
    }
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
        let name = entry
            .name()
            .map_err(|e| invalid(format!("A bundle entry name is unreadable: {e}")))?
            .to_string();
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
    Ok((archive, packages, settings, digest))
}

/// Creates a file that did not exist, never following or truncating one
/// that does. A leftover regular file (an interrupted export's `.part`) is
/// removed first; anything else is refused.
fn create_fresh(path: &Path) -> std::io::Result<File> {
    let create = || {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    };
    match create() {
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let leftover = std::fs::symlink_metadata(path)?;
            if !leftover.file_type().is_file() {
                return Err(error);
            }
            std::fs::remove_file(path)?;
            create()
        }
        other => other,
    }
}

/// Puts a finished file in place without replacing one that appeared in the
/// meantime. A hard link fails if the target exists; where links are not
/// supported (some USB drives) it falls back to a checked rename.
fn publish_no_clobber(partial: &Path, target: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(partial, target) {
        Ok(()) => {
            let _ = std::fs::remove_file(partial);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(error),
        Err(_) => {
            if std::fs::symlink_metadata(target).is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "a file with that name appeared while the bundle was written",
                ));
            }
            std::fs::rename(partial, target)
        }
    }
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
                create_fresh(&partial).map_err(|e| io_error("Could not create the bundle", e))?;
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
                if let Err(error) = publish_no_clobber(&partial, &target) {
                    let _ = std::fs::remove_file(&partial);
                    return Err(io_error("Could not finalise the bundle", error));
                }
                Ok(target)
            }
            Err(error) => {
                let _ = std::fs::remove_file(&partial);
                Err(error)
            }
        }
    }

    fn write_recipe(
        &self,
        dest_dir: &Path,
        base_name: &str,
        recipe_json: &str,
    ) -> AppResult<PathBuf> {
        std::fs::create_dir_all(dest_dir)
            .map_err(|e| io_error("Could not create the destination folder", e))?;
        let mut counter = 1;
        loop {
            let target = if counter == 1 {
                dest_dir.join(format!("{base_name}.json"))
            } else {
                dest_dir.join(format!("{base_name}-{counter}.json"))
            };
            // Created only if absent, so an existing file is never replaced.
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
            {
                Ok(mut file) => {
                    let written = file
                        .write_all(recipe_json.as_bytes())
                        .and_then(|()| file.sync_all());
                    if let Err(error) = written {
                        let _ = std::fs::remove_file(&target);
                        return Err(io_error("Could not write the recipe file", error));
                    }
                    return Ok(target);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    counter += 1;
                    if counter > 1000 {
                        return Err(io_error("Could not find a free file name", error));
                    }
                }
                Err(error) => return Err(io_error("Could not create the recipe file", error)),
            }
        }
    }

    fn peek_bundle(&self, path: &Path) -> AppResult<BundlePeek> {
        let (mut archive, packages, settings, digest) = open(path)?;
        let recipe_json = read_recipe(&mut archive)?;
        let mut settings_for: Vec<String> = settings.into_iter().map(|(_, id, _)| id).collect();
        settings_for.sort();
        settings_for.dedup();
        Ok(BundlePeek {
            recipe_json,
            packages: packages.into_iter().map(|(_, hash)| hash).collect(),
            settings_for,
            digest,
        })
    }

    fn package_manifests(&self, package: &Path) -> AppResult<Vec<PackageManifest>> {
        Self::manifests_in(package)
            .map_err(|why| invalid(format!("A package's manifest could not be read: {why}")))
    }

    fn discard_scratch(&self, dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    fn read_bundle(&self, path: &Path, extract_dir: &Path) -> AppResult<BundleContents> {
        std::fs::create_dir_all(extract_dir)
            .map_err(|e| io_error("Could not create the working folder", e))?;
        // Everything is read from one copy, hashed as it is made, so the
        // digest covers exactly the bytes imported even if the original is
        // rewritten in place meanwhile. It is scratch space like the rest.
        let snapshot_path = extract_dir.join("bundle.snapshot");
        let digest = {
            let mut original =
                File::open(path).map_err(|e| io_error("Could not open the bundle", e))?;
            let mut snapshot = create_fresh(&snapshot_path)
                .map_err(|e| io_error("Could not copy the bundle", e))?;
            hash_file(&mut original, Some(&mut snapshot))?
        };
        let snapshot =
            File::open(&snapshot_path).map_err(|e| io_error("Could not open the bundle", e))?;
        let (mut archive, entries, setting_entries, digest) = classify(snapshot, digest)?;
        let recipe_json = read_recipe(&mut archive)?;

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
            // Scratch copies are deleted after the import, so they are not
            // flushed to disk one by one; the install keeps its own copy.
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
        let mut skipped_settings = Vec::new();
        let mut settings_total: u64 = 0;
        for (index, unique_id, relative_path) in setting_entries {
            let entry = archive
                .by_index(index)
                .map_err(|e| invalid(format!("A settings file could not be read: {e}")))?;
            settings_total += entry.size().min(MAX_SETTING_BYTES + 1);
            if settings_total > MAX_SETTINGS_BYTES {
                return Err(invalid("The bundle's settings files are too large"));
            }
            let mut bytes = Vec::new();
            entry
                .take(MAX_SETTING_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| invalid(format!("A settings file could not be read: {e}")))?;
            // A file that cannot be shared is left out and reported, so the
            // mods still install (older bundles allowed larger files).
            if bytes.len() as u64 > MAX_SETTING_BYTES {
                skipped_settings.push((
                    unique_id,
                    format!(
                        "{relative_path} is larger than the 256 KB a shared settings file may be."
                    ),
                ));
                continue;
            }
            if let Some(problem) = manager_core::settings_privacy::json_problem(&bytes) {
                skipped_settings.push((unique_id, format!("{relative_path} {problem}.")));
                continue;
            }
            settings.push(BundledSetting {
                unique_id,
                relative_path,
                bytes,
            });
        }
        drop(archive);
        let _ = std::fs::remove_file(&snapshot_path);
        Ok(BundleContents {
            recipe_json,
            packages,
            settings,
            skipped_settings,
            digest,
        })
    }
}

impl ZipBundleArchive {
    /// The mods a package's manifests declare, read without extracting it.
    fn manifests_in(package: &Path) -> Result<Vec<PackageManifest>, String> {
        let file = File::open(package).map_err(|e| format!("it could not be opened: {e}"))?;
        let mut archive = ZipArchive::new(file).map_err(|e| format!("it is not a zip: {e}"))?;
        if archive.len() > MAX_ENTRY_COUNT {
            return Err("it has too many entries".to_string());
        }
        let mut out = Vec::new();
        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|e| format!("an entry is unreadable: {e}"))?;
            let is_manifest = entry.name().ok().is_some_and(|name| {
                name.rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|file| file.eq_ignore_ascii_case("manifest.json"))
            });
            if !is_manifest || entry.is_dir() {
                continue;
            }
            if out.len() >= MAX_MANIFESTS || entry.size() > MAX_MANIFEST_BYTES {
                return Err("its manifests are too many or too large".to_string());
            }
            let mut text = String::new();
            entry
                .take(MAX_MANIFEST_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|e| format!("a manifest is not readable text: {e}"))?;
            let manifest = manager_core::manifest::parse_manifest(&text)?;
            out.push(PackageManifest {
                unique_id: manifest.unique_id.as_str().to_string(),
                version: manifest.version,
            });
        }
        if out.is_empty() {
            return Err("it has no manifest.json".to_string());
        }
        Ok(out)
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
