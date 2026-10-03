//! Adopting mods from a Mods folder the manager does not own.
//!
//! The folder is only read. Each chosen mod folder is packaged as an archive
//! and installed into a new profile through the normal install pipeline, so
//! it is checked, stored and journaled like any install, and the whole
//! adoption is one change set. The original folder is never changed or
//! removed: anything not adopted stays exactly where it was, and so does
//! everything that was.

use crate::api::dto::{
    AdoptComponentDto, AdoptableModDto, AdoptionFailureDto, AdoptionResultDto, AdoptionScanDto,
    UnknownEntryDto,
};
use crate::error::{AppError, AppResult};
use crate::ports::mods_folder::{FolderScan, ModsFolderPort};
use crate::ports::repositories::GameInstallationRepository;
use crate::services::{
    ChangeSets, ModsService, OperationsService, PackagesService, ProfilesService,
};
use manager_core::ids::{GameInstallationId, OperationId, ProfileId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

pub struct AdoptionService {
    game_repo: Arc<dyn GameInstallationRepository>,
    folder: Arc<dyn ModsFolderPort>,
    packages: Arc<PackagesService>,
    profiles: Arc<ProfilesService>,
    mods: Arc<ModsService>,
    operations: Arc<OperationsService>,
    change_sets: Arc<ChangeSets>,
    work_dir: PathBuf,
}

impl AdoptionService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        game_repo: Arc<dyn GameInstallationRepository>,
        folder: Arc<dyn ModsFolderPort>,
        packages: Arc<PackagesService>,
        profiles: Arc<ProfilesService>,
        mods: Arc<ModsService>,
        operations: Arc<OperationsService>,
        change_sets: Arc<ChangeSets>,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            game_repo,
            folder,
            packages,
            profiles,
            mods,
            operations,
            change_sets,
            work_dir,
        }
    }

    fn mods_dir(&self, game_id: &GameInstallationId) -> AppResult<PathBuf> {
        let game = self
            .game_repo
            .get_game(game_id)?
            .ok_or_else(|| AppError::validation("GAME_NOT_FOUND", "That game is not registered"))?;
        Ok(game.canonical_root.join("Mods"))
    }

    fn to_dto(&self, scan: &FolderScan) -> AppResult<AdoptionScanDto> {
        // Folders per UniqueID, to name duplicates.
        let mut by_id: HashMap<String, Vec<String>> = HashMap::new();
        for m in &scan.mods {
            for c in &m.manifests {
                by_id
                    .entry(c.unique_id.to_lowercase())
                    .or_default()
                    .push(m.folder.clone());
            }
        }
        let mut mods = Vec::new();
        for m in &scan.mods {
            let mut duplicate_of: Vec<String> = m
                .manifests
                .iter()
                .flat_map(|c| by_id[&c.unique_id.to_lowercase()].clone())
                .filter(|f| f != &m.folder)
                .collect();
            duplicate_of.sort();
            duplicate_of.dedup();
            let mut stored = "none";
            let mut locally_modified = Vec::new();
            for c in &m.manifests {
                let candidates = self.packages.stored_with_unique_id(&c.unique_id, None)?;
                if let Some(same) = candidates.iter().find(|s| s.version == c.version) {
                    // Compare files, so a known mod changed locally is flagged.
                    let differs =
                        manager_core::ids::ArtifactHash::parse(same.artifact_hash.clone())
                            .ok()
                            .and_then(|hash| self.packages.get_artifact_path(&hash).ok())
                            .and_then(|path| {
                                self.folder
                                    .differences_from_archive(&scan.mods_dir, &m.folder, &path)
                                    .ok()
                            });
                    match differs {
                        Some(files) if files.is_empty() => stored = "exact",
                        Some(files) => {
                            stored = "same_version";
                            locally_modified = files;
                        }
                        None => stored = "same_version",
                    }
                } else if !candidates.is_empty() && stored == "none" {
                    stored = "other_version";
                }
            }
            mods.push(AdoptableModDto {
                folder: m.folder.clone(),
                components: m
                    .manifests
                    .iter()
                    .map(|c| AdoptComponentDto {
                        unique_id: c.unique_id.clone(),
                        name: c.name.clone(),
                        version: c.version.clone(),
                        author: c.author.clone(),
                        update_keys: c.update_keys.clone(),
                        requires: c.requires.clone(),
                    })
                    .collect(),
                size_bytes: m.size_bytes,
                file_count: m.file_count,
                has_settings: m.has_settings,
                problems: m.problems.clone(),
                duplicate_of,
                stored: stored.to_string(),
                locally_modified,
            });
        }
        Ok(AdoptionScanDto {
            mods_dir: scan.mods_dir.to_string_lossy().to_string(),
            total_bytes: mods.iter().map(|m| m.size_bytes).sum(),
            mods,
            unknown: scan
                .unknown
                .iter()
                .map(|u| UnknownEntryDto {
                    name: u.name.clone(),
                    size_bytes: u.size_bytes,
                    is_folder: u.is_folder,
                    reason: u.reason.clone(),
                })
                .collect(),
            runtime: scan.runtime.clone(),
            manager_markers: scan.manager_markers.clone(),
            fingerprint: scan.fingerprint.clone(),
        })
    }

    /// What the game's Mods folder holds. Changes nothing.
    pub fn scan(&self, game_id: &GameInstallationId) -> AppResult<AdoptionScanDto> {
        let dir = self.mods_dir(game_id)?;
        let scan = self.folder.scan(&dir)?;
        self.to_dto(&scan)
    }

    /// Copies the chosen mod folders into a new profile, as reviewed. The
    /// folder must be exactly as it was when scanned (`fingerprint`).
    pub fn adopt(
        &self,
        game_id: &GameInstallationId,
        profile_name: &str,
        folders: &[String],
        fingerprint: &str,
    ) -> AppResult<AdoptionResultDto> {
        let dir = self.mods_dir(game_id)?;
        let scan = self.folder.scan(&dir)?;
        if scan.fingerprint != fingerprint {
            return Err(AppError::validation(
                "ADOPTION_PLAN_STALE",
                "The Mods folder changed since you reviewed it, so nothing was adopted. Scan it again.",
            ));
        }
        let dto = self.to_dto(&scan)?;
        let chosen: HashSet<&str> = folders.iter().map(String::as_str).collect();
        for m in &dto.mods {
            if chosen.contains(m.folder.as_str())
                && m.duplicate_of.iter().any(|d| chosen.contains(d.as_str()))
            {
                return Err(AppError::validation(
                    "ADOPTION_DUPLICATES",
                    format!(
                        "{} and {} hold the same mod. Choose one of them.",
                        m.folder,
                        m.duplicate_of.join(", ")
                    ),
                ));
            }
        }
        let adoptable: HashSet<&str> = dto.mods.iter().map(|m| m.folder.as_str()).collect();
        if let Some(bad) = folders.iter().find(|f| !adoptable.contains(f.as_str())) {
            return Err(AppError::validation(
                "ADOPTION_NOT_A_MOD",
                format!("{bad} is not a mod folder that can be adopted"),
            ));
        }
        if folders.is_empty() {
            return Err(AppError::validation(
                "ADOPTION_NOTHING_CHOSEN",
                "Choose at least one mod to adopt",
            ));
        }

        let profile = self.profiles.create_profile(
            game_id,
            profile_name,
            Some(&format!("Adopted from {}", dir.display())),
        )?;
        let profile_id = ProfileId::from_str(&profile.id)
            .map_err(|e| AppError::internal("Profile id was not readable", e.to_string()))?;
        let journal = self.change_sets.begin(
            &profile_id,
            &format!("Adopting {} mod folder(s)", folders.len()),
            "adoption",
            "",
            None,
            folders,
        )?;
        let work = self.work_dir.join(journal.to_string());
        let mut adopted = Vec::new();
        let mut failed = Vec::new();
        for (index, folder) in folders.iter().enumerate() {
            let step = || -> AppResult<()> {
                let archive = self.folder.package(&dir, folder, &work)?;
                let preview = self.mods.prepare_install(&profile_id, &archive)?;
                let id = OperationId::from_str(&preview.operation_id)
                    .map_err(|e| AppError::internal("Operation id", e.to_string()))?;
                if !preview.blockers.is_empty() {
                    let _ = self.operations.cancel_operation(&id);
                    return Err(AppError::validation(
                        "ADOPTION_BLOCKED",
                        preview.blockers.join(" "),
                    ));
                }
                self.operations.commit_operation(&id)?;
                Ok(())
            };
            match step() {
                Ok(()) => {
                    adopted.push(folder.clone());
                    let _ = self.change_sets.part_done(&journal, index as u32, None);
                }
                Err(e) => {
                    let _ = self
                        .change_sets
                        .part_done(&journal, index as u32, Some(&e.summary));
                    failed.push(AdoptionFailureDto {
                        folder: folder.clone(),
                        reason: e.summary,
                    });
                }
            }
        }
        self.folder.discard(&work);
        self.change_sets.finish(
            &journal,
            &failed
                .iter()
                .map(|f| format!("{}: {}", f.folder, f.reason))
                .collect::<Vec<_>>(),
        )?;
        let mut left_in_place: Vec<String> = dto
            .mods
            .iter()
            .map(|m| m.folder.clone())
            .filter(|f| !adopted.contains(f))
            .chain(dto.unknown.iter().map(|u| u.name.clone()))
            .collect();
        left_in_place.sort();
        Ok(AdoptionResultDto {
            profile_id: profile.id,
            profile_name: profile.name,
            adopted,
            failed,
            left_in_place,
        })
    }
}
