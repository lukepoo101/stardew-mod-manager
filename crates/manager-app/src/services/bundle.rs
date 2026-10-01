//! Moving a profile between people or computers as one file.
//!
//! A bundle is a recipe plus the packages it names. Importing builds a new
//! profile beside the existing ones, so nothing that already works is touched;
//! anything that cannot be installed is reported, never skipped silently.

use crate::api::dto::{
    BundleComponentDto, BundleExportDto, BundleFailureDto, BundleImportDto, BundlePreviewDto,
    UnfinishedCopyDto,
};
use crate::error::{AppError, AppResult};
use crate::ports::bundle::BundleArchivePort;
use crate::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, PackageCatalogRepository, ProfileRepository,
    SmapiRepository,
};
use crate::services::mods::ModsService;
use crate::services::operations::OperationsService;
use crate::services::packages::PackagesService;
use crate::services::profiles::ProfilesService;
use crate::services::toggle::ToggleService;
use manager_core::ids::{ArtifactHash, GameInstallationId, OperationId, ProfileId};
use manager_core::recipe::{ProfileRecipe, RecipeComponent, RecipeGame};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

pub struct BundleService {
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    game_repo: Arc<dyn GameInstallationRepository>,
    smapi_repo: Arc<dyn SmapiRepository>,
    packages: Arc<PackagesService>,
    profiles: Arc<ProfilesService>,
    mods: Arc<ModsService>,
    operations: Arc<OperationsService>,
    toggle: Arc<ToggleService>,
    archive: Arc<dyn BundleArchivePort>,
    work_dir: PathBuf,
    files: Option<Arc<dyn crate::ports::deployed_files::DeployedFilesPort>>,
    copy_journal: Option<Arc<dyn crate::ports::repositories::PreferencesRepository>>,
}

/// A duplicate that has been started but not finished: what it was copied
/// from, as it was then, so it can be completed later even if the source
/// has changed since.
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingCopy {
    source_id: String,
    source_name: String,
    recipe: ProfileRecipe,
}

fn pending_copy_key(profile_id: &ProfileId) -> String {
    format!("copy_pending:{profile_id}")
}

/// A profile as a recipe plus the package files it was built from.
struct Snapshot {
    profile_name: String,
    recipe: ProfileRecipe,
    packages: Vec<(ArtifactHash, PathBuf)>,
    /// Mods whose package is no longer stored.
    missing_packages: Vec<String>,
}

/// Keeps a profile name usable as a file name on every platform.
fn file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "profile".to_string()
    } else {
        trimmed.chars().take(60).collect()
    }
}

impl BundleService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        game_repo: Arc<dyn GameInstallationRepository>,
        smapi_repo: Arc<dyn SmapiRepository>,
        packages: Arc<PackagesService>,
        profiles: Arc<ProfilesService>,
        mods: Arc<ModsService>,
        operations: Arc<OperationsService>,
        toggle: Arc<ToggleService>,
        archive: Arc<dyn BundleArchivePort>,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            profile_repo,
            deployment_repo,
            package_repo,
            game_repo,
            smapi_repo,
            packages,
            profiles,
            mods,
            operations,
            toggle,
            archive,
            work_dir,
            files: None,
            copy_journal: None,
        }
    }

    /// Records each duplicate before it is filled in, so one that is
    /// interrupted can be found and finished.
    pub fn with_copy_journal(
        mut self,
        preferences: Arc<dyn crate::ports::repositories::PreferencesRepository>,
    ) -> Self {
        self.copy_journal = Some(preferences);
        self
    }

    /// Lets bundles carry chosen mods' settings files.
    pub fn with_settings(
        mut self,
        files: Arc<dyn crate::ports::deployed_files::DeployedFilesPort>,
    ) -> Self {
        self.files = Some(files);
        self
    }

    /// Mod folder by UniqueID (lowercase) for a profile.
    fn folders(&self, profile_id: &ProfileId) -> AppResult<HashMap<String, String>> {
        let mut out = HashMap::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            let Some(component) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            if let Some(deployment) = self.deployment_repo.get_deployment(&pc.deployment_id)? {
                out.insert(
                    component.unique_id.as_str().to_lowercase(),
                    deployment.root_relative_path,
                );
            }
        }
        Ok(out)
    }

    /// The profile as a recipe, with the retained package files it was built
    /// from and the names of mods whose package is no longer stored.
    fn snapshot(&self, profile_id: &ProfileId) -> AppResult<Snapshot> {
        let profile = self
            .profile_repo
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        let game = self.game_repo.get_game(&profile.game_installation_id)?;
        let smapi_version = self
            .smapi_repo
            .get_smapi_installation(&profile.game_installation_id)?
            .map(|installation| installation.release_version);

        let mut components = Vec::new();
        let mut hashes: BTreeMap<String, ArtifactHash> = BTreeMap::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            let Some(component) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            let deployment = self.deployment_repo.get_deployment(&pc.deployment_id)?;
            let artifact_hash = deployment
                .as_ref()
                .map(|d| d.artifact_hash.clone())
                .unwrap_or_else(|| component.artifact_hash.clone());
            hashes.insert(artifact_hash.as_str().to_string(), artifact_hash.clone());
            components.push(RecipeComponent {
                unique_id: component.unique_id.to_string(),
                name: component.name,
                author: component.author,
                version: component.version,
                enabled: pc.enabled,
                artifact_hash: artifact_hash.as_str().to_string(),
                optional: false,
            });
        }

        let mut packages = Vec::new();
        let mut missing_hashes = BTreeSet::new();
        for (key, hash) in &hashes {
            match self.packages.get_artifact_path(hash) {
                Ok(path) if self.packages.has_artifact(hash) => packages.push((hash.clone(), path)),
                _ => {
                    missing_hashes.insert(key.clone());
                }
            }
        }
        let missing_packages: Vec<String> = components
            .iter()
            .filter(|c| missing_hashes.contains(&c.artifact_hash))
            .map(|c| c.name.clone())
            .collect();

        let recipe = ProfileRecipe::new(
            profile.name.clone(),
            chrono::Utc::now().to_rfc3339(),
            RecipeGame {
                storefront: game
                    .map(|g| format!("{:?}", g.storefront))
                    .unwrap_or_default(),
                smapi_version,
            },
            components,
        );
        Ok(Snapshot {
            profile_name: profile.name,
            recipe,
            packages,
            missing_packages,
        })
    }

    /// Writes the profile, with the packages it was built from, to a new file.
    pub fn export_bundle(
        &self,
        profile_id: &ProfileId,
        dest_dir: &Path,
    ) -> AppResult<BundleExportDto> {
        self.export_bundle_including(profile_id, dest_dir, &[])
    }

    /// As [`Self::export_bundle`], also carrying the settings files of the
    /// mods the user chose.
    pub fn export_bundle_including(
        &self,
        profile_id: &ProfileId,
        dest_dir: &Path,
        settings_for: &[String],
    ) -> AppResult<BundleExportDto> {
        let Snapshot {
            profile_name,
            recipe,
            packages,
            missing_packages,
        } = self.snapshot(profile_id)?;
        let recipe_json = recipe
            .to_json()
            .map_err(|e| AppError::internal("The recipe could not be written", e))?;

        let stem = format!(
            "{}-{}",
            file_stem(&profile_name),
            chrono::Utc::now().format("%Y%m%d")
        );
        let mut settings = Vec::new();
        let mut settings_included = Vec::new();
        if !settings_for.is_empty() {
            let files = self.files.as_ref().ok_or_else(|| {
                AppError::internal("Settings cannot be read here", "no deployed-files port")
            })?;
            let folders = self.folders(profile_id)?;
            for unique_id in settings_for {
                let Some(folder) = folders.get(&unique_id.to_lowercase()) else {
                    continue;
                };
                let found = files.read_configs(profile_id, folder)?;
                if !found.is_empty() {
                    settings_included.push(unique_id.clone());
                }
                settings.extend(found.into_iter().map(|(relative_path, bytes)| {
                    crate::ports::bundle::BundledSetting {
                        unique_id: unique_id.clone(),
                        relative_path,
                        bytes,
                    }
                }));
            }
        }
        let path =
            self.archive
                .write_bundle(dest_dir, &stem, &recipe_json, &packages, &settings)?;
        Ok(BundleExportDto {
            path: path.to_string_lossy().to_string(),
            component_count: recipe.components.len(),
            package_count: packages.len(),
            missing_packages,
            settings_included,
        })
    }

    fn parse_recipe(json: &str) -> AppResult<ProfileRecipe> {
        ProfileRecipe::parse(json)
            .map_err(|errors| AppError::validation("INVALID_RECIPE", errors.join(" ")))
    }

    /// Reads what a bundle holds without installing or changing anything.
    pub fn inspect_bundle(&self, path: &Path) -> AppResult<BundlePreviewDto> {
        let peek = self.archive.peek_bundle(path)?;
        let (recipe_json, included) = (peek.recipe_json, peek.packages);
        let recipe = Self::parse_recipe(&recipe_json)?;
        let included: HashSet<String> = included
            .iter()
            .map(|hash| hash.as_str().to_lowercase())
            .collect();
        let referenced: HashSet<String> = recipe
            .components
            .iter()
            .map(|c| c.artifact_hash.to_lowercase())
            .collect();

        let components: Vec<BundleComponentDto> = recipe
            .components
            .iter()
            .map(|c| BundleComponentDto {
                unique_id: c.unique_id.clone(),
                name: c.name.clone(),
                version: c.version.clone(),
                enabled: c.enabled,
                package_included: included.contains(&c.artifact_hash.to_lowercase()),
            })
            .collect();
        let missing_packages = components
            .iter()
            .filter(|c| !c.package_included)
            .map(|c| c.name.clone())
            .collect();
        let mut warnings = Vec::new();
        let unused = included.iter().filter(|h| !referenced.contains(*h)).count();
        if unused > 0 {
            warnings.push(format!(
                "{unused} package(s) in the bundle are not used by the recipe and will be ignored."
            ));
        }
        Ok(BundlePreviewDto {
            profile_name: recipe.profile_name,
            generated_at: recipe.generated_at,
            components,
            missing_packages,
            warnings,
            settings_for: peek.settings_for,
        })
    }

    /// Builds a new profile from a bundle. The existing profiles are untouched.
    /// Installs the given packages into a new, empty profile through the normal
    /// install engine, then leaves disabled what the recipe says was disabled.
    /// Returns what was installed, what was left disabled and what failed.
    fn materialise(
        &self,
        profile_id: &ProfileId,
        recipe: &ProfileRecipe,
        packages: &[(ArtifactHash, PathBuf)],
        missing_reason: &str,
    ) -> AppResult<(Vec<String>, Vec<String>, Vec<BundleFailureDto>)> {
        // Names per package, so a failure can say which mods it concerns.
        let mut names_for: HashMap<String, Vec<String>> = HashMap::new();
        for component in &recipe.components {
            names_for
                .entry(component.artifact_hash.to_lowercase())
                .or_default()
                .push(component.name.clone());
        }
        let label = |hash: &ArtifactHash| {
            names_for
                .get(&hash.as_str().to_lowercase())
                .map(|names| names.join(", "))
                .unwrap_or_else(|| hash.as_str().to_string())
        };

        let wanted: HashSet<String> = names_for.keys().cloned().collect();
        // Packages already in the profile (from an earlier, interrupted run)
        // are not installed twice.
        let present: HashSet<String> = self
            .deployment_repo
            .list_deployments_for_profile(profile_id)?
            .into_iter()
            .map(|d| d.artifact_hash.as_str().to_lowercase())
            .collect();
        let mut pending: Vec<_> = packages
            .iter()
            .filter(|package| {
                let hash = package.0.as_str().to_lowercase();
                wanted.contains(&hash) && !present.contains(&hash)
            })
            .collect();

        let mut failures = Vec::new();
        let mut installed = Vec::new();
        let mut last_reasons: HashMap<String, String> = HashMap::new();

        // Dependencies have to be present before what needs them, and the order
        // is not written down, so keep trying what is left until a pass installs
        // nothing new.
        loop {
            let mut progressed = false;
            let mut still_pending = Vec::new();
            for package in pending {
                let preview = match self.mods.prepare_install(profile_id, &package.1) {
                    Ok(preview) => preview,
                    Err(error) => {
                        failures.push(BundleFailureDto {
                            name: label(&package.0),
                            reason: error.summary.clone(),
                        });
                        continue;
                    }
                };
                let operation_id = OperationId::from_str(&preview.operation_id).map_err(|e| {
                    AppError::internal("Operation id was not readable", e.to_string())
                })?;
                if !preview.blockers.is_empty() || !preview.dependencies_satisfied {
                    let reason = if preview.blockers.is_empty() {
                        "It needs a mod that is not installed yet".to_string()
                    } else {
                        preview.blockers.join(" ")
                    };
                    last_reasons.insert(package.0.as_str().to_string(), reason);
                    let _ = self.operations.cancel_operation(&operation_id);
                    still_pending.push(package);
                    continue;
                }
                match self.operations.commit_operation(&operation_id) {
                    Ok(_) => {
                        progressed = true;
                        installed.extend(
                            preview
                                .detected_components
                                .iter()
                                .map(|c| format!("{} {}", c.name, c.version)),
                        );
                    }
                    Err(error) => failures.push(BundleFailureDto {
                        name: label(&package.0),
                        reason: error.summary.clone(),
                    }),
                }
            }
            pending = still_pending;
            if pending.is_empty() || !progressed {
                break;
            }
        }
        for package in pending {
            failures.push(BundleFailureDto {
                name: label(&package.0),
                reason: last_reasons
                    .get(package.0.as_str())
                    .cloned()
                    .unwrap_or_else(|| "It could not be installed".to_string()),
            });
        }

        // Anything the recipe lists but the bundle did not carry.
        let carried: HashSet<String> = packages
            .iter()
            .map(|p| p.0.as_str().to_lowercase())
            .collect();
        for component in &recipe.components {
            let hash = component.artifact_hash.to_lowercase();
            if !carried.contains(&hash) && !present.contains(&hash) {
                failures.push(BundleFailureDto {
                    name: component.name.clone(),
                    reason: missing_reason.to_string(),
                });
            }
        }

        // Restore the original enabled state.
        let disabled_ids: HashSet<String> = recipe
            .components
            .iter()
            .filter(|c| !c.enabled)
            .map(|c| c.unique_id.clone())
            .collect();
        let mut disabled = Vec::new();
        if !disabled_ids.is_empty() {
            for pc in self.deployment_repo.list_profile_components(profile_id)? {
                let Some(component) = self
                    .package_repo
                    .get_package_component(&pc.package_component_id)?
                else {
                    continue;
                };
                if disabled_ids.contains(component.unique_id.as_str()) && pc.enabled {
                    match self.toggle.set_enabled(&pc.id, false) {
                        Ok(()) => disabled.push(component.name.clone()),
                        Err(error) => failures.push(BundleFailureDto {
                            name: component.name.clone(),
                            reason: format!(
                                "Installed, but could not be left disabled: {}",
                                error.summary
                            ),
                        }),
                    }
                }
            }
        }

        installed.sort();
        disabled.sort();
        Ok((installed, disabled, failures))
    }

    /// Makes an independent copy of a profile: a new profile with its own id
    /// and folder, the same mods and versions installed from the retained
    /// packages, and the same enabled state. The source is not changed and
    /// stays active; nothing links the two afterwards except the description.
    pub fn clone_profile(
        &self,
        source_id: &ProfileId,
        new_name: &str,
    ) -> AppResult<BundleImportDto> {
        let source = self
            .profile_repo
            .get_profile(source_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        let snapshot = self.snapshot(source_id)?;
        let profile = self.profiles.create_profile_copy(
            &source,
            new_name,
            Some(&format!("Copy of {}", source.name)),
        )?;
        let profile_id = ProfileId::from_str(&profile.id)
            .map_err(|e| AppError::internal("Profile id was not readable", e.to_string()))?;
        if let Some(journal) = &self.copy_journal {
            let pending = PendingCopy {
                source_id: source_id.to_string(),
                source_name: source.name.clone(),
                recipe: snapshot.recipe.clone(),
            };
            let json = serde_json::to_string(&pending)
                .map_err(|e| AppError::internal("Could not record the copy", e.to_string()))?;
            journal.set_preference(&pending_copy_key(&profile_id), &json)?;
        }
        self.fill_copy(
            &profile_id,
            profile.name,
            source_id,
            &snapshot.recipe,
            &snapshot.packages,
        )
    }

    /// Installs what a copy still lacks, restores the enabled state and
    /// copies the settings, then marks the copy finished.
    fn fill_copy(
        &self,
        profile_id: &ProfileId,
        profile_name: String,
        source_id: &ProfileId,
        recipe: &ProfileRecipe,
        packages: &[(ArtifactHash, PathBuf)],
    ) -> AppResult<BundleImportDto> {
        let (installed, disabled, failures) = self.materialise(
            profile_id,
            recipe,
            packages,
            "The package this mod was installed from is no longer stored",
        )?;
        // A copy should behave like the original, so its settings come too.
        let mut settings_applied = Vec::new();
        if let (Some(files), Some(_)) = (&self.files, self.profile_repo.get_profile(source_id)?) {
            let source_folders = self.folders(source_id)?;
            let copy_folders = self.folders(profile_id)?;
            for (unique_id, folder) in &source_folders {
                let Some(target) = copy_folders.get(unique_id) else {
                    continue;
                };
                let settings = files.read_configs(source_id, folder)?;
                if !settings.is_empty() {
                    files.write_files(profile_id, target, &settings)?;
                    settings_applied.push(unique_id.clone());
                }
            }
            settings_applied.sort();
        }
        if let Some(journal) = &self.copy_journal {
            // Empty means "no pending copy", as elsewhere in the preferences store.
            journal.set_preference(&pending_copy_key(profile_id), "")?;
        }
        Ok(BundleImportDto {
            profile_id: profile_id.to_string(),
            profile_name,
            installed,
            disabled,
            failures,
            settings_applied,
        })
    }

    /// Duplicates that were started but not finished, for example because
    /// the app closed part-way. Copies whose profile no longer exists are
    /// forgotten.
    pub fn unfinished_copies(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Vec<UnfinishedCopyDto>> {
        let Some(journal) = &self.copy_journal else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for profile in self.profile_repo.list_profiles(game_id)? {
            let Some(json) = journal.get_preference(&pending_copy_key(&profile.id))? else {
                continue;
            };
            let Ok(pending) = serde_json::from_str::<PendingCopy>(&json) else {
                continue;
            };
            out.push(UnfinishedCopyDto {
                profile_id: profile.id.to_string(),
                profile_name: profile.name,
                source_name: pending.source_name,
                expected_mods: pending.recipe.components.len(),
            });
        }
        out.sort_by(|a, b| a.profile_name.cmp(&b.profile_name));
        Ok(out)
    }

    /// Finishes an interrupted duplicate from what was recorded when it
    /// started: installs only the mods it lacks, then restores the enabled
    /// state and settings.
    pub fn finish_copy(&self, profile_id: &ProfileId) -> AppResult<BundleImportDto> {
        let journal = self.copy_journal.as_ref().ok_or_else(|| {
            AppError::validation("COPY_NOT_PENDING", "This profile is not an unfinished copy")
        })?;
        let pending: PendingCopy = journal
            .get_preference(&pending_copy_key(profile_id))?
            .and_then(|json| serde_json::from_str(&json).ok())
            .ok_or_else(|| {
                AppError::validation("COPY_NOT_PENDING", "This profile is not an unfinished copy")
            })?;
        let profile = self
            .profile_repo
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        let source_id = ProfileId::from_str(&pending.source_id)
            .map_err(|e| AppError::internal("Source profile id was not readable", e.to_string()))?;
        let mut packages = Vec::new();
        let mut seen = HashSet::new();
        for component in &pending.recipe.components {
            let key = component.artifact_hash.to_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            let Ok(hash) = ArtifactHash::parse(key) else {
                continue;
            };
            if let Ok(path) = self.packages.get_artifact_path(&hash) {
                if self.packages.has_artifact(&hash) {
                    packages.push((hash, path));
                }
            }
        }
        self.fill_copy(
            profile_id,
            profile.name,
            &source_id,
            &pending.recipe,
            &packages,
        )
    }

    pub fn import_bundle(
        &self,
        path: &Path,
        game_id: &GameInstallationId,
        profile_name: &str,
    ) -> AppResult<BundleImportDto> {
        let work = self
            .work_dir
            .join(format!("import-{}", uuid::Uuid::new_v4()));
        let result = self.import_from(path, game_id, profile_name, &work);
        // The extracted copies are scratch space either way.
        self.archive.discard_scratch(&work);
        result
    }

    fn import_from(
        &self,
        path: &Path,
        game_id: &GameInstallationId,
        profile_name: &str,
        work: &Path,
    ) -> AppResult<BundleImportDto> {
        let contents = self.archive.read_bundle(path, work)?;
        let recipe = Self::parse_recipe(&contents.recipe_json)?;

        let profile =
            self.profiles
                .create_profile(game_id, profile_name, Some("Imported from a bundle"))?;
        let profile_id = ProfileId::from_str(&profile.id)
            .map_err(|e| AppError::internal("Profile id was not readable", e.to_string()))?;

        let packages: Vec<(ArtifactHash, PathBuf)> = contents
            .packages
            .iter()
            .map(|p| (p.hash.clone(), p.path.clone()))
            .collect();
        let (installed, disabled, mut failures) = self.materialise(
            &profile_id,
            &recipe,
            &packages,
            "The bundle does not include this mod's package",
        )?;

        // Settings go into the freshly installed mods only.
        let mut settings_applied = Vec::new();
        if !contents.settings.is_empty() {
            if let Some(files) = &self.files {
                let folders = self.folders(&profile_id)?;
                let mut by_mod: BTreeMap<String, Vec<(String, Vec<u8>)>> = BTreeMap::new();
                for setting in &contents.settings {
                    by_mod
                        .entry(setting.unique_id.to_lowercase())
                        .or_default()
                        .push((setting.relative_path.clone(), setting.bytes.clone()));
                }
                for (unique_id, entries) in by_mod {
                    match folders.get(&unique_id) {
                        Some(folder) => match files.write_files(&profile_id, folder, &entries) {
                            Ok(()) => settings_applied.push(unique_id),
                            Err(error) => failures.push(BundleFailureDto {
                                name: unique_id,
                                reason: format!("Its settings were not applied: {}", error.summary),
                            }),
                        },
                        None => failures.push(BundleFailureDto {
                            name: unique_id,
                            reason:
                                "Its settings were not applied because the mod was not installed"
                                    .to_string(),
                        }),
                    }
                }
            }
        }
        Ok(BundleImportDto {
            profile_id: profile.id,
            profile_name: profile.name,
            installed,
            disabled,
            failures,
            settings_applied,
        })
    }
}
