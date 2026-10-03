//! Reads a Mods folder the manager does not own, for adoption, and packages
//! one of its mod folders as an archive. Reading never changes anything;
//! links are never followed, so nothing outside the folder is reached.
#![allow(clippy::result_large_err)]

use crate::platform::shared::inspector::BUNDLED_MOD_FOLDERS;
use manager_app::error::{AppError, AppResult};
use manager_app::ports::mods_folder::{
    FolderScan, ModsFolderPort, ScannedManifest, ScannedMod, ScannedUnknown,
};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// Files other mod managers leave in folders they deploy.
const MANAGER_MARKERS: &[(&str, &str)] = &[
    ("__folder_managed_by_vortex", "Vortex"),
    ("vortex.deployment.json", "Vortex"),
    ("vortex.deployment.msgpack", "Vortex"),
];

/// How deep to look for manifests inside one mod folder.
const MAX_DEPTH: usize = 6;

pub struct FilesystemModsFolder;

struct Walked {
    files: Vec<(String, u64)>,
    manifests: Vec<String>,
    links: Vec<String>,
    too_deep: bool,
    markers: Vec<String>,
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Walked) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some((_, manager)) = MANAGER_MARKERS
            .iter()
            .find(|(marker, _)| name.eq_ignore_ascii_case(marker))
        {
            out.markers.push(format!("{manager} ({relative})"));
        }
        if meta.file_type().is_symlink() {
            out.links.push(relative);
        } else if meta.is_dir() {
            if depth >= MAX_DEPTH {
                out.too_deep = true;
            } else {
                walk(root, &path, depth + 1, out);
            }
        } else if meta.is_file() {
            if name.eq_ignore_ascii_case("manifest.json") {
                out.manifests.push(relative.clone());
            }
            out.files.push((relative, meta.len()));
        }
    }
}

impl ModsFolderPort for FilesystemModsFolder {
    fn scan(&self, mods_dir: &Path) -> AppResult<FolderScan> {
        let entries = std::fs::read_dir(mods_dir)
            .map_err(|e| AppError::filesystem("Could not read the Mods folder", e.to_string()))?;
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        let mut scan = FolderScan {
            mods_dir: mods_dir.to_path_buf(),
            mods: Vec::new(),
            unknown: Vec::new(),
            runtime: Vec::new(),
            manager_markers: Vec::new(),
            fingerprint: String::new(),
        };
        let mut hasher = Sha256::new();
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if let Some((_, manager)) = MANAGER_MARKERS
                .iter()
                .find(|(marker, _)| name.eq_ignore_ascii_case(marker))
            {
                scan.manager_markers.push(format!("{manager} ({name})"));
            }
            if meta.file_type().is_symlink() {
                scan.unknown.push(ScannedUnknown {
                    name: name.clone(),
                    size_bytes: 0,
                    is_folder: false,
                    reason: "It is a link to somewhere else, which is not followed".into(),
                });
                hasher.update(format!("link:{name}\n"));
                continue;
            }
            if !meta.is_dir() {
                scan.unknown.push(ScannedUnknown {
                    name: name.clone(),
                    size_bytes: meta.len(),
                    is_folder: false,
                    reason: "A file, not a mod folder".into(),
                });
                hasher.update(format!("file:{name}:{}\n", meta.len()));
                continue;
            }
            if BUNDLED_MOD_FOLDERS.contains(&name.as_str()) {
                scan.runtime.push(name);
                continue;
            }
            let mut walked = Walked {
                files: Vec::new(),
                manifests: Vec::new(),
                links: Vec::new(),
                too_deep: false,
                markers: Vec::new(),
            };
            walk(&path, &path, 0, &mut walked);
            for (file, size) in &walked.files {
                hasher.update(format!("{name}/{file}:{size}\n"));
            }
            scan.manager_markers
                .extend(walked.markers.iter().map(|m| format!("{m} in {name}")));
            let size_bytes = walked.files.iter().map(|(_, size)| size).sum();
            let mut problems = Vec::new();
            let mut manifests = Vec::new();
            for manifest_path in &walked.manifests {
                let parsed = std::fs::read_to_string(path.join(manifest_path))
                    .map_err(|e| e.to_string())
                    .and_then(|text| manager_core::manifest::parse_manifest(&text));
                match parsed {
                    Ok(manifest) => manifests.push(ScannedManifest {
                        path: manifest_path.clone(),
                        unique_id: manifest.unique_id.to_string(),
                        name: manifest.name,
                        version: manifest.version,
                        author: manifest.author,
                        requires: manifest
                            .content_pack_for
                            .iter()
                            .map(|h| h.unique_id.to_string())
                            .chain(
                                manifest
                                    .dependencies
                                    .iter()
                                    .filter(|d| d.is_required)
                                    .map(|d| d.unique_id.to_string()),
                            )
                            .collect(),
                        update_keys: manifest.update_keys,
                    }),
                    Err(error) => {
                        problems.push(format!("{manifest_path} could not be read: {error}"))
                    }
                }
            }
            if !walked.links.is_empty() {
                problems.push(format!(
                    "Links are not followed or copied: {}",
                    walked.links.join(", ")
                ));
            }
            if walked.too_deep {
                problems.push("Some folders are nested too deeply to look inside".into());
            }
            if manifests.is_empty() {
                scan.unknown.push(ScannedUnknown {
                    name,
                    size_bytes,
                    is_folder: true,
                    reason: if problems.is_empty() {
                        "No manifest.json, so it is not a recognisable mod".into()
                    } else {
                        problems.join("; ")
                    },
                });
                continue;
            }
            let has_settings = walked.files.iter().any(|(file, _)| {
                file.rsplit('/')
                    .next()
                    .is_some_and(|n| n.eq_ignore_ascii_case("config.json"))
            });
            scan.mods.push(ScannedMod {
                folder: name,
                manifests,
                size_bytes,
                file_count: walked.files.len(),
                has_settings,
                problems,
            });
        }
        scan.fingerprint = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(scan)
    }

    fn differences_from_archive(
        &self,
        mods_dir: &Path,
        folder: &str,
        archive: &Path,
    ) -> AppResult<Vec<String>> {
        let source = mods_dir.join(folder);
        let mut walked = Walked {
            files: Vec::new(),
            manifests: Vec::new(),
            links: Vec::new(),
            too_deep: false,
            markers: Vec::new(),
        };
        walk(&source, &source, 0, &mut walked);
        let is_settings = |path: &str| {
            path.rsplit('/')
                .next()
                .is_some_and(|n| n.eq_ignore_ascii_case("config.json"))
        };
        let read_err = |e: String| AppError::filesystem("The stored package could not be read", e);
        let mut zip =
            zip::ZipArchive::new(File::open(archive).map_err(|e| read_err(e.to_string()))?)
                .map_err(|e| read_err(e.to_string()))?;
        // The archive's own top folder is dropped, so names line up.
        let mut packaged = std::collections::HashMap::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| read_err(e.to_string()))?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().replace('\\', "/");
            let inner = name
                .split_once('/')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or(name);
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes)
                .map_err(|e| read_err(e.to_string()))?;
            packaged.insert(inner, Sha256::digest(&bytes).to_vec());
        }
        let mut differs = Vec::new();
        for (file, _) in &walked.files {
            if is_settings(file) {
                continue;
            }
            let bytes = std::fs::read(source.join(file))
                .map_err(|e| AppError::filesystem("A mod file could not be read", e.to_string()))?;
            if packaged.get(file) != Some(&Sha256::digest(&bytes).to_vec()) {
                differs.push(file.clone());
            }
        }
        Ok(differs)
    }

    fn discard(&self, work_dir: &Path) {
        let _ = std::fs::remove_dir_all(work_dir);
    }

    fn package(&self, mods_dir: &Path, folder: &str, dest_dir: &Path) -> AppResult<PathBuf> {
        let plain = !folder.is_empty()
            && Path::new(folder)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
            && !folder.contains(['/', '\\']);
        if !plain {
            return Err(AppError::validation(
                "ADOPT_FOLDER_INVALID",
                "That is not a folder directly inside the Mods folder",
            ));
        }
        let source = mods_dir.join(folder);
        let meta = std::fs::symlink_metadata(&source)
            .map_err(|e| AppError::filesystem("The mod folder could not be read", e.to_string()))?;
        if !meta.is_dir() {
            return Err(AppError::validation(
                "ADOPT_FOLDER_INVALID",
                "That is not a folder directly inside the Mods folder",
            ));
        }
        std::fs::create_dir_all(dest_dir).map_err(|e| {
            AppError::filesystem("Could not prepare the adoption folder", e.to_string())
        })?;
        let mut walked = Walked {
            files: Vec::new(),
            manifests: Vec::new(),
            links: Vec::new(),
            too_deep: false,
            markers: Vec::new(),
        };
        walk(&source, &source, 0, &mut walked);
        let target = dest_dir.join(format!("{}.zip", sanitise(folder)));
        let write = || -> std::io::Result<()> {
            let mut zip = ZipWriter::new(File::create(&target)?);
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .large_file(true);
            for (file, _) in &walked.files {
                zip.start_file(format!("{folder}/{file}"), options)?;
                let mut input = File::open(source.join(file))?;
                std::io::copy(&mut input, &mut zip)?;
            }
            zip.finish()?.flush()
        };
        write().map_err(|e| {
            let _ = std::fs::remove_file(&target);
            AppError::filesystem("The mod folder could not be packaged", e.to_string())
        })?;
        Ok(target)
    }
}

/// Copies a profile's live mods into `dest_dir/Mods` (or `Mods-2`, … when
/// that exists), as plain folders SMAPI can load without this manager.
/// Links are not followed; nothing in the profile changes.
pub fn copy_plain_mods(profile_mods: &Path, dest_dir: &Path) -> AppResult<PathBuf> {
    let mut target = dest_dir.join("Mods");
    let mut counter = 2;
    while target.exists() {
        target = dest_dir.join(format!("Mods-{counter}"));
        counter += 1;
    }
    let copy = || -> std::io::Result<()> {
        std::fs::create_dir_all(&target)?;
        let mut walked = Walked {
            files: Vec::new(),
            manifests: Vec::new(),
            links: Vec::new(),
            too_deep: false,
            markers: Vec::new(),
        };
        walk(profile_mods, profile_mods, 0, &mut walked);
        for (file, _) in &walked.files {
            let to = target.join(file);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(profile_mods.join(file), to)?;
        }
        Ok(())
    };
    copy().map_err(|e| AppError::filesystem("The mods could not be copied", e.to_string()))?;
    Ok(target)
}

fn sanitise(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn manifest(id: &str) -> String {
        format!(
            r#"{{"Name":"{id}","Author":"a","Version":"1.0.0","UniqueID":"{id}","EntryDll":"{id}.dll","UpdateKeys":["Nexus:1"]}}"#
        )
    }

    #[test]
    fn scanning_reads_without_changing_and_sorts_what_it_finds() {
        let tmp = tempfile::tempdir().unwrap();
        let mods = tmp.path().join("Mods");
        write(&mods.join("Good/manifest.json"), &manifest("A.Good"));
        write(&mods.join("Good/config.json"), "{}");
        write(&mods.join("Pack/Inner/manifest.json"), &manifest("B.Inner"));
        write(&mods.join("Broken/manifest.json"), "{ not json");
        write(&mods.join("Loose/readme.txt"), "hi");
        write(&mods.join("notes.txt"), "file");
        write(
            &mods.join("ConsoleCommands/manifest.json"),
            &manifest("SMAPI.ConsoleCommands"),
        );
        write(&mods.join("Good/__folder_managed_by_vortex"), "");
        let before = std::fs::read_dir(&mods).unwrap().count();

        let scan = FilesystemModsFolder.scan(&mods).unwrap();
        assert_eq!(
            scan.mods
                .iter()
                .map(|m| m.folder.as_str())
                .collect::<Vec<_>>(),
            vec!["Good", "Pack"]
        );
        let good = &scan.mods[0];
        assert_eq!(good.manifests[0].unique_id, "A.Good");
        assert_eq!(good.manifests[0].update_keys, vec!["Nexus:1"]);
        assert!(good.has_settings);
        assert_eq!(scan.mods[1].manifests[0].path, "Inner/manifest.json");
        let unknown: Vec<_> = scan.unknown.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(unknown, vec!["Broken", "Loose", "notes.txt"]);
        assert!(scan.unknown[0].reason.contains("could not be read"));
        assert_eq!(scan.runtime, vec!["ConsoleCommands"]);
        assert!(scan.manager_markers.iter().any(|m| m.contains("Vortex")));
        // Nothing was changed, and the same folder scans the same.
        assert_eq!(std::fs::read_dir(&mods).unwrap().count(), before);
        assert_eq!(
            FilesystemModsFolder.scan(&mods).unwrap().fingerprint,
            scan.fingerprint
        );
        write(&mods.join("Good/new.txt"), "x");
        assert_ne!(
            FilesystemModsFolder.scan(&mods).unwrap().fingerprint,
            scan.fingerprint
        );
    }

    #[test]
    fn a_plain_copy_never_overwrites_and_skips_nothing_but_links() {
        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path().join("profile-mods");
        write(&profile.join("Good/manifest.json"), &manifest("A.Good"));
        write(&profile.join("Good/config.json"), "{}");
        let out = tmp.path().join("out");
        std::fs::create_dir_all(out.join("Mods")).unwrap();
        let written = copy_plain_mods(&profile, &out).unwrap();
        assert_eq!(written, out.join("Mods-2"));
        assert!(written.join("Good/config.json").is_file());
        assert!(profile.join("Good/manifest.json").is_file());
    }

    #[test]
    fn packaging_puts_the_folder_at_the_archive_root_and_refuses_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let mods = tmp.path().join("Mods");
        write(&mods.join("Good/manifest.json"), &manifest("A.Good"));
        write(&mods.join("Good/assets/a.png"), "png");
        let zip_path = FilesystemModsFolder
            .package(&mods, "Good", &tmp.path().join("out"))
            .unwrap();
        let mut archive = zip::ZipArchive::new(File::open(&zip_path).unwrap()).unwrap();
        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["Good/assets/a.png", "Good/manifest.json"]);
        assert!(FilesystemModsFolder
            .package(&mods, "../Mods", &tmp.path().join("out"))
            .is_err());
        assert!(FilesystemModsFolder
            .package(&mods, "Good/assets", &tmp.path().join("out"))
            .is_err());
    }
}
