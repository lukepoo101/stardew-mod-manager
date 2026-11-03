//! Moving a profile between people or computers as one file.
//!
//! A bundle is a recipe plus the packages it names. Importing builds a new
//! profile beside the existing ones, so nothing that already works is touched;
//! anything that cannot be installed is reported, never skipped silently.

use crate::api::dto::{
    BundleComponentDto, BundleExportDto, BundleFailureDto, BundleImportDto, BundlePreviewDto,
    BundleProgressDto, DeletedProfileDto, SettingsComparisonDto, UnfinishedCopyDto,
};
use crate::error::{AppError, AppResult};
use crate::ports::bundle::BundleArchivePort;
use crate::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, PackageCatalogRepository, ProfileRepository,
    SmapiRepository,
};
use crate::ports::runtime_layout::GameRuntimePort;
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
use std::sync::{Arc, Mutex};

/// What a bundle records besides the mods themselves.
#[derive(Debug, Clone, Default)]
pub struct BundleExtras {
    /// UniqueIDs of mods whose files differ from their package.
    pub locally_modified: Vec<String>,
    /// Source links the user added, by lower-case UniqueID.
    pub source_urls: HashMap<String, String>,
    pub frozen: Option<manager_core::recipe::FrozenInfo>,
    /// Mods (by UniqueID) whose flagged settings fields (paths, email
    /// addresses, account details) the user chose to share. Keys, tokens
    /// and passwords are never shared.
    pub keep_flagged_settings: Vec<String>,
}

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
    /// Settings backups, for recreating a restore point's settings.
    config_backups: Option<Arc<dyn crate::ports::config_backups::ConfigBackupsPort>>,
    copy_journal: Option<Arc<dyn crate::ports::repositories::PreferencesRepository>>,
    import_references: Option<crate::services::ReferenceRecipes>,
    trash: Option<Arc<dyn crate::ports::profile_folders::ProfileFolderPort>>,
    /// The host platform, for recipe entries marked for other systems.
    runtime: Arc<dyn GameRuntimePort>,
    progress: Arc<Mutex<Option<BundleProgressDto>>>,
}

/// Joins messages into sentences, so each ends with a full stop.
fn sentences<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    parts
        .into_iter()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.ends_with(['.', '!', '?']) {
                part.to_string()
            } else {
                format!("{part}.")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether two manifest versions are the same version, read as SMAPI does.
fn same_version(a: &str, b: &str) -> bool {
    use manager_core::manifest::SmapiVersion;
    match (SmapiVersion::parse(a.trim()), SmapiVersion::parse(b.trim())) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.trim() == b.trim(),
    }
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
        runtime: Arc<dyn GameRuntimePort>,
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
            config_backups: None,
            copy_journal: None,
            import_references: None,
            trash: None,
            runtime,
            progress: Arc::new(Mutex::new(None)),
        }
    }

    /// This computer's operating system, as recipes name it.
    fn host_os(&self) -> &'static str {
        self.runtime.operating_system().as_key()
    }

    /// Whether the recipe marks a mod for other operating systems only.
    fn not_for_here(&self, component: &RecipeComponent) -> bool {
        !component.platforms.is_empty()
            && !component
                .platforms
                .iter()
                .any(|p| p.eq_ignore_ascii_case(self.host_os()))
    }

    /// The packages never installed here: any that holds a mod marked for
    /// other operating systems. Worked out the same way for the preview and
    /// the import, so what the preview shows is what happens.
    fn excluded_packages(&self, recipe: &ProfileRecipe) -> HashSet<String> {
        recipe
            .components
            .iter()
            .filter(|c| self.not_for_here(c) && !c.artifact_hash.is_empty())
            .map(|c| c.artifact_hash.to_lowercase())
            .collect()
    }

    /// The names of the other recipe entries that come in the same package.
    fn package_mates(recipe: &ProfileRecipe, component: &RecipeComponent) -> Vec<String> {
        if component.artifact_hash.is_empty() {
            return Vec::new();
        }
        recipe
            .components
            .iter()
            .filter(|c| {
                c.artifact_hash
                    .eq_ignore_ascii_case(&component.artifact_hash)
                    && !c.unique_id.eq_ignore_ascii_case(&component.unique_id)
            })
            .map(|c| c.name.clone())
            .collect()
    }

    /// How far the bundle import that is running has got, if one is.
    pub fn import_progress(&self) -> Option<BundleProgressDto> {
        self.progress.lock().ok().and_then(|p| p.clone())
    }

    fn set_progress(&self, value: Option<BundleProgressDto>) {
        if let Ok(mut progress) = self.progress.lock() {
            *progress = value;
        }
    }

    /// Lets deleted profiles still in the trash be brought back.
    pub fn with_trash(
        mut self,
        folders: Arc<dyn crate::ports::profile_folders::ProfileFolderPort>,
    ) -> Self {
        self.trash = Some(folders);
        self
    }

    fn deletion_records(
        &self,
    ) -> AppResult<Vec<(String, crate::services::profile_deletion::DeletionRecord)>> {
        let Some(trash) = &self.trash else {
            return Ok(Vec::new());
        };
        Ok(trash
            .list_deletion_records()?
            .into_iter()
            .filter_map(|(entry, json)| Some((entry, serde_json::from_str(&json).ok()?)))
            .collect())
    }

    /// Deleted profiles of this game that can still be brought back.
    pub fn deleted_profiles(
        &self,
        game_id: &GameInstallationId,
    ) -> AppResult<Vec<DeletedProfileDto>> {
        let mut out: Vec<DeletedProfileDto> = self
            .deletion_records()?
            .into_iter()
            .filter(|(_, record)| record.game_installation_id == game_id.to_string())
            .map(|(entry, record)| DeletedProfileDto {
                entry,
                name: record.name,
                deleted_at: record.deleted_at,
                mod_count: record.mods.len(),
            })
            .collect();
        out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
        Ok(out)
    }

    /// Recreates a deleted profile from its record: the same mods at the same
    /// versions and enabled state from the stored archives (only intact ones;
    /// a missing archive is reported, never substituted), with each mod's
    /// settings copied back from the deleted folder. It keeps its old id (and
    /// carries on from its old revision), so saves linked to it are linked
    /// again, and the undo copies of its earlier removals move back with it.
    /// The trashed folder itself stays in the trash for the rest of its
    /// retention, marked as brought back: it can hold what was not brought
    /// back (files of a mod whose package is gone, mods' own data files).
    pub fn bring_back(&self, entry: &str) -> AppResult<BundleImportDto> {
        let trash = self.trash.as_ref().ok_or_else(|| {
            AppError::internal("The trash cannot be read here", "no profile folders port")
        })?;
        let record = self
            .deletion_records()?
            .into_iter()
            .find(|(known, _)| known == entry)
            .map(|(_, record)| record)
            .ok_or_else(|| {
                AppError::validation(
                    "DELETED_PROFILE_NOT_FOUND",
                    "That deleted profile is no longer in the trash",
                )
            })?;
        let game_id = GameInstallationId::from_str(&record.game_installation_id)
            .map_err(|e| AppError::internal("Game id was not readable", e.to_string()))?;
        let taken = self
            .profile_repo
            .list_profiles(&game_id)?
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(&record.name));
        let name = if taken {
            format!("{} (brought back)", record.name)
        } else {
            record.name.clone()
        };
        // The same id as before, so the saves linked to it, its restore
        // points and its last working setup are its own again.
        let old_id = ProfileId::from_str(&record.profile_id).ok();
        let profile = match &old_id {
            Some(old_id) => self.profiles.recreate_deleted_profile(
                &game_id,
                old_id,
                &name,
                record.description.as_deref(),
                record.revision,
            )?,
            None => self
                .profiles
                .create_profile(&game_id, &name, record.description.as_deref())?,
        };
        let profile_id = ProfileId::from_str(&profile.id)
            .map_err(|e| AppError::internal("Profile id was not readable", e.to_string()))?;
        if let Some(preferences) = &self.copy_journal {
            // State that would stop it being rebuilt, or would list it as
            // something else, does not come back with it.
            crate::services::freeze::forget_freeze(&**preferences, &profile_id)?;
            preferences.set_preference(&pending_copy_key(&profile_id), "")?;
        }

        let recipe = ProfileRecipe::new(
            record.name.clone(),
            chrono::Utc::now().to_rfc3339(),
            RecipeGame::default(),
            record
                .mods
                .iter()
                .map(|m| RecipeComponent {
                    unique_id: m.unique_id.clone(),
                    name: m.name.clone(),
                    author: String::new(),
                    version: m.version.clone(),
                    enabled: m.enabled,
                    artifact_hash: m.artifact_hash.clone(),
                    optional: false,
                    version_rule: None,
                    group: None,
                    manual: None,
                    client_only: false,
                    note: None,
                    settings: Vec::new(),
                    update_keys: Vec::new(),
                    source_url: None,
                    requires: Vec::new(),
                    locally_modified: false,
                    platforms: Vec::new(),
                })
                .collect(),
        );
        let mut packages = Vec::new();
        let mut seen = HashSet::new();
        for m in &record.mods {
            let key = m.artifact_hash.to_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            let Ok(hash) = ArtifactHash::parse(key) else {
                continue;
            };
            if self.packages.has_artifact(&hash) && self.packages.verify_artifact(&hash)? {
                packages.push((hash.clone(), self.packages.get_artifact_path(&hash)?));
            }
        }
        let (installed, disabled, mut failures) = self.materialise(
            &profile_id,
            &recipe,
            &packages,
            "Its exact package is no longer stored intact, so nothing was put in its place",
            &format!("Bringing back \"{}\"", record.name),
        )?;

        // Settings come back from the deleted folder.
        let mut settings_applied = Vec::new();
        if let Some(files) = &self.files {
            let folders = self.folders(&profile_id)?;
            for m in &record.mods {
                let Some(target) = folders.get(&m.unique_id.to_lowercase()) else {
                    continue;
                };
                let settings = trash.trashed_configs(entry, &m.folder)?;
                if settings.is_empty() {
                    continue;
                }
                match files.write_files(&profile_id, target, &settings) {
                    Ok(()) => settings_applied.push(m.unique_id.clone()),
                    Err(error) => failures.push(BundleFailureDto {
                        name: m.name.clone(),
                        reason: format!("Its settings were not restored: {}", error.summary),
                    }),
                }
            }
            settings_applied.sort();
            settings_applied.dedup();
        }
        // Undoing an earlier removal reads the removed copy from the
        // profile's own folder, so those copies come back with it when the id
        // is the same. Losing them only loses the undo, so it is reported.
        if old_id == Some(profile_id) {
            if let Err(error) = trash.move_recovery_copies(entry, &profile_id) {
                failures.push(BundleFailureDto {
                    name: "Undo copies of earlier removals".to_string(),
                    reason: format!(
                        "They stay in the old folder in the trash: {}",
                        error.summary
                    ),
                });
            }
        }
        // No longer offered for bringing back; the folder stays for the rest
        // of its time in the trash, and storage cleanup labels it.
        let brought_back = crate::services::profile_deletion::BroughtBackRecord {
            profile_id: profile.id.clone(),
            name: profile.name.clone(),
            deleted_at: record.deleted_at.clone(),
            brought_back_at: chrono::Utc::now().to_rfc3339(),
        };
        let json = serde_json::to_string(&brought_back)
            .map_err(|e| AppError::internal("Could not record the bring back", e.to_string()))?;
        trash.mark_brought_back(entry, &json)?;
        Ok(BundleImportDto {
            profile_id: profile.id,
            profile_name: profile.name,
            installed,
            disabled,
            failures,
            settings_applied,
            declined_optional: Vec::new(),
            reference_attached: false,
            not_for_this_computer: Vec::new(),
            installed_with_package: Vec::new(),
        })
    }

    /// Keeps each imported bundle's mod list as the new profile's group
    /// reference, so anything that could not be installed stays listed as a
    /// difference instead of only appearing in the import result.
    pub fn with_import_references(
        mut self,
        preferences: Arc<dyn crate::ports::repositories::PreferencesRepository>,
    ) -> Self {
        self.import_references = Some(crate::services::ReferenceRecipes::new(preferences));
        self
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

    /// Lets a profile recreated from a restore point get the settings the
    /// point kept.
    pub fn with_config_backups(
        mut self,
        backups: Arc<dyn crate::ports::config_backups::ConfigBackupsPort>,
    ) -> Self {
        self.config_backups = Some(backups);
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
            let requires = component
                .manifest
                .content_pack_for
                .iter()
                .map(|host| host.unique_id.to_string())
                .chain(
                    component
                        .manifest
                        .dependencies
                        .iter()
                        .filter(|d| d.is_required)
                        .map(|d| d.unique_id.to_string()),
                )
                .collect();
            components.push(RecipeComponent {
                unique_id: component.unique_id.to_string(),
                name: component.name,
                author: component.author,
                version: component.version,
                enabled: pc.enabled,
                artifact_hash: artifact_hash.as_str().to_string(),
                optional: false,
                version_rule: None,
                group: None,
                manual: None,
                client_only: false,
                note: None,
                settings: Vec::new(),
                update_keys: component.manifest.update_keys.clone(),
                source_url: None,
                requires,
                locally_modified: false,
                platforms: Vec::new(),
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
                // As the browser-built recipe writes it ("steam", "manual").
                storefront: game
                    .and_then(|g| serde_json::to_value(g.storefront).ok())
                    .and_then(|v| v.as_str().map(str::to_string))
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
        self.export_bundle_with(profile_id, dest_dir, settings_for, &[])
    }

    /// As [`Self::export_bundle_including`], also marking the mods in
    /// `optional` (by UniqueID) as optional for recipients.
    pub fn export_bundle_with(
        &self,
        profile_id: &ProfileId,
        dest_dir: &Path,
        settings_for: &[String],
        optional: &[String],
    ) -> AppResult<BundleExportDto> {
        self.export_bundle_full(
            profile_id,
            dest_dir,
            settings_for,
            optional,
            &BundleExtras::default(),
        )
    }

    /// As [`Self::export_bundle_with`], also recording what else a move to
    /// another computer should keep: the freeze, source links the user
    /// added, and which mods were changed outside the manager.
    pub fn export_bundle_full(
        &self,
        profile_id: &ProfileId,
        dest_dir: &Path,
        settings_for: &[String],
        optional: &[String],
        extras: &BundleExtras,
    ) -> AppResult<BundleExportDto> {
        let Snapshot {
            profile_name,
            mut recipe,
            packages,
            missing_packages,
        } = self.snapshot(profile_id)?;
        for component in &mut recipe.components {
            let id = component.unique_id.to_lowercase();
            component.optional = optional
                .iter()
                .any(|o| o.eq_ignore_ascii_case(&component.unique_id));
            component.locally_modified = extras
                .locally_modified
                .iter()
                .any(|m| m.eq_ignore_ascii_case(&component.unique_id));
            component.source_url = extras.source_urls.get(&id).cloned();
        }
        recipe.frozen = extras.frozen.clone();
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
        let mut settings_redacted = Vec::new();
        let mut settings_skipped = Vec::new();
        if !settings_for.is_empty() {
            let files = self.files.as_ref().ok_or_else(|| {
                AppError::internal("Settings cannot be read here", "no deployed-files port")
            })?;
            let folders = self.folders(profile_id)?;
            let names: HashMap<String, String> = recipe
                .components
                .iter()
                .map(|c| (c.unique_id.to_lowercase(), c.name.clone()))
                .collect();
            for unique_id in settings_for {
                let id = unique_id.to_lowercase();
                let Some(folder) = folders.get(&id) else {
                    continue;
                };
                let name = names.get(&id).cloned().unwrap_or_else(|| unique_id.clone());
                // The same privacy rules as a published collection: keys,
                // tokens and passwords never leave, and paths, email
                // addresses and account details only when the user says so.
                let keep_flagged = extras
                    .keep_flagged_settings
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(unique_id));
                let mut carried = false;
                for (relative_path, bytes) in files.read_configs(profile_id, folder)? {
                    if bytes.len() > manager_core::recipe::MAX_SETTING_BYTES {
                        settings_skipped
                            .push(format!("{name} {relative_path} (larger than 256 KB)"));
                        continue;
                    }
                    if let Some(problem) = manager_core::settings_privacy::json_problem(&bytes) {
                        settings_skipped.push(format!("{name} {relative_path} ({problem})"));
                        continue;
                    }
                    let shared =
                        manager_core::settings_privacy::redact_settings(&bytes, keep_flagged);
                    let mut taken = Vec::new();
                    if !shared.blocked.is_empty() {
                        taken.push(format!("{} (never shared)", shared.blocked.join(", ")));
                    }
                    if !shared.removed.is_empty() {
                        taken.push(shared.removed.join(", "));
                    }
                    if !taken.is_empty() {
                        settings_redacted.push(format!(
                            "{name} {relative_path}: left out {}",
                            taken.join("; ")
                        ));
                    }
                    carried = true;
                    settings.push(crate::ports::bundle::BundledSetting {
                        unique_id: unique_id.clone(),
                        relative_path,
                        bytes: shared.bytes,
                    });
                }
                if carried {
                    settings_included.push(unique_id.clone());
                }
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
            settings_redacted,
            settings_skipped,
        })
    }

    /// Saves a recipe (a plain export or a collection revision) the user is
    /// sharing into a folder they chose, named after the profile or
    /// collection. Only a valid recipe is written, and never over a file.
    pub fn save_recipe_file(
        &self,
        dest_dir: &Path,
        file_name: &str,
        recipe_json: &str,
    ) -> AppResult<PathBuf> {
        Self::parse_recipe(recipe_json)?;
        let stem = file_name
            .trim()
            .strip_suffix(".json")
            .unwrap_or(file_name.trim());
        self.archive
            .write_recipe(dest_dir, &file_stem(stem), recipe_json)
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
        let excluded = self.excluded_packages(&recipe);
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
            .map(|c| {
                let not_here = self.not_for_here(c);
                let mates = Self::package_mates(&recipe, c);
                // A mod in one package with a mod for another system cannot
                // be installed without it, so it is left out too.
                let left_out_reason =
                    (!not_here && excluded.contains(&c.artifact_hash.to_lowercase())).then(|| {
                        let others: Vec<&str> = recipe
                            .components
                            .iter()
                            .filter(|o| {
                                self.not_for_here(o)
                                    && o.artifact_hash.eq_ignore_ascii_case(&c.artifact_hash)
                            })
                            .map(|o| o.name.as_str())
                            .collect();
                        format!(
                            "It comes in one package with {}, which is not for this computer.",
                            others.join(", ")
                        )
                    });
                BundleComponentDto {
                    unique_id: c.unique_id.clone(),
                    name: c.name.clone(),
                    version: c.version.clone(),
                    enabled: c.enabled,
                    package_included: included.contains(&c.artifact_hash.to_lowercase()),
                    optional: c.optional,
                    platforms: c.platforms.clone(),
                    not_for_this_computer: not_here,
                    shares_package_with: mates,
                    left_out_reason,
                }
            })
            .collect();
        // Mods the import will not install although they are meant for this
        // computer: their package is missing, or it is one that is never
        // installed here. Going ahead without them needs a yes.
        let missing_packages = components
            .iter()
            .filter(|c| {
                !c.not_for_this_computer && (!c.package_included || c.left_out_reason.is_some())
            })
            .map(|c| c.name.clone())
            .collect();
        let mut warnings = Vec::new();
        let unused = included.iter().filter(|h| !referenced.contains(*h)).count();
        if unused > 0 {
            warnings.push(format!(
                "{unused} package(s) in the bundle are not used by the recipe and will be ignored."
            ));
        }
        // Requirements the bundle cannot provide: not listed, or listed
        // without its package. Worked out before anything is created.
        let carried: HashSet<String> = recipe
            .components
            .iter()
            .filter(|c| {
                let hash = c.artifact_hash.to_lowercase();
                included.contains(&hash) && !excluded.contains(&hash)
            })
            .map(|c| c.unique_id.to_lowercase())
            .collect();
        let mut unresolved_requirements = Vec::new();
        for component in recipe.components.iter().filter(|c| !self.not_for_here(c)) {
            for required in &component.requires {
                if !carried.contains(&required.to_lowercase()) {
                    unresolved_requirements.push(format!("{} needs {}", component.name, required));
                }
            }
        }
        Ok(BundlePreviewDto {
            locally_modified: recipe
                .components
                .iter()
                .filter(|c| c.locally_modified)
                .map(|c| c.name.clone())
                .collect(),
            frozen_at: recipe.frozen.as_ref().map(|f| f.frozen_at.clone()),
            frozen_reason: recipe.frozen.as_ref().map(|f| f.reason.clone()),
            unresolved_requirements,
            profile_name: recipe.profile_name,
            generated_at: recipe.generated_at,
            components,
            missing_packages,
            warnings,
            settings_for: peek.settings_for,
            operating_system: self.host_os().to_string(),
            bundle_digest: peek.digest,
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
        change: &str,
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
        let pending: Vec<_> = packages
            .iter()
            .filter(|package| {
                let hash = package.0.as_str().to_lowercase();
                wanted.contains(&hash) && !present.contains(&hash)
            })
            .collect();
        let mut pending = Self::requirements_first(recipe, pending);
        let total = pending.len();
        let mut done = 0;

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
                self.set_progress(Some(BundleProgressDto {
                    done,
                    total,
                    current: label(&package.0),
                }));
                let preview = match self.mods.prepare_install(profile_id, &package.1) {
                    Ok(preview) => preview,
                    Err(error) => {
                        done += 1;
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
                        "It needs a mod that is not installed yet.".to_string()
                    } else {
                        sentences(preview.blockers.iter().map(String::as_str))
                    };
                    last_reasons.insert(package.0.as_str().to_string(), reason);
                    let _ = self.operations.cancel_operation(&operation_id);
                    still_pending.push(package);
                    continue;
                }
                done += 1;
                match self.operations.commit_operation(&operation_id) {
                    Ok(_) => {
                        if let Some(preferences) = &self.copy_journal {
                            let _ = crate::services::operation_labels::label_operation(
                                &**preferences,
                                &operation_id,
                                change,
                            );
                        }
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
        self.set_progress(None);
        for package in pending {
            failures.push(BundleFailureDto {
                name: label(&package.0),
                reason: last_reasons
                    .get(package.0.as_str())
                    .cloned()
                    .unwrap_or_else(|| "It could not be installed.".to_string()),
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
        // By UniqueID whatever its case, as the identity check matches them.
        let disabled_ids: HashSet<String> = recipe
            .components
            .iter()
            .filter(|c| !c.enabled)
            .map(|c| c.unique_id.to_lowercase())
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
                if disabled_ids.contains(&component.unique_id.as_str().to_lowercase()) && pc.enabled
                {
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

    /// Packages in an order where what a mod requires comes before it, as
    /// far as the recipe says, so each is prepared once rather than tried,
    /// put back and tried again.
    fn requirements_first<'p>(
        recipe: &ProfileRecipe,
        mut pending: Vec<&'p (ArtifactHash, PathBuf)>,
    ) -> Vec<&'p (ArtifactHash, PathBuf)> {
        let mut provides: HashMap<String, HashSet<String>> = HashMap::new();
        let mut needs: HashMap<String, HashSet<String>> = HashMap::new();
        for c in &recipe.components {
            let hash = c.artifact_hash.to_lowercase();
            provides
                .entry(hash.clone())
                .or_default()
                .insert(c.unique_id.to_lowercase());
            needs
                .entry(hash)
                .or_default()
                .extend(c.requires.iter().map(|r| r.to_lowercase()));
        }
        let in_bundle: HashSet<String> = pending
            .iter()
            .flat_map(|p| {
                provides
                    .get(&p.0.as_str().to_lowercase())
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
        let mut ordered = Vec::with_capacity(pending.len());
        let mut placed: HashSet<String> = HashSet::new();
        while !pending.is_empty() {
            let before = pending.len();
            pending.retain(|package| {
                let hash = package.0.as_str().to_lowercase();
                let ready = needs.get(&hash).is_none_or(|wanted| {
                    wanted
                        .iter()
                        .all(|id| placed.contains(id) || !in_bundle.contains(id))
                });
                if ready {
                    placed.extend(provides.get(&hash).cloned().unwrap_or_default());
                    ordered.push(*package);
                }
                !ready
            });
            if pending.len() == before {
                // A loop or a requirement provided by the same package: the
                // install engine's own checks decide.
                ordered.append(&mut pending);
            }
        }
        ordered
    }

    /// Every package must hold exactly the mods the recipe names it for, at
    /// the versions the recipe gives, or the recipe and packages do not
    /// belong together and nothing is imported.
    fn check_identities(
        &self,
        recipe: &ProfileRecipe,
        packages: &[crate::ports::bundle::BundledPackage],
    ) -> AppResult<()> {
        let mut named: HashMap<String, Vec<&RecipeComponent>> = HashMap::new();
        for c in &recipe.components {
            if !c.artifact_hash.is_empty() {
                named
                    .entry(c.artifact_hash.to_lowercase())
                    .or_default()
                    .push(c);
            }
        }
        let refuse = |detail: String| {
            AppError::validation(
                "BUNDLE_IDENTITY_MISMATCH",
                format!(
                    "{detail} The recipe and the packages do not belong together, so the bundle is damaged or has been altered. Nothing was imported."
                ),
            )
        };
        for package in packages {
            let Some(components) = named.get(&package.hash.as_str().to_lowercase()) else {
                continue;
            };
            let found = self.archive.package_manifests(&package.path)?;
            let held = found
                .iter()
                .map(|m| format!("{} {}", m.unique_id, m.version))
                .collect::<Vec<_>>()
                .join(", ");
            for c in components {
                let matching = found
                    .iter()
                    .find(|m| m.unique_id.eq_ignore_ascii_case(&c.unique_id));
                match matching {
                    None => {
                        return Err(refuse(format!(
                            "The recipe lists {} {} ({}), but the package it names holds {held}.",
                            c.name, c.version, c.unique_id
                        )))
                    }
                    Some(m) if !same_version(&m.version, &c.version) => {
                        return Err(refuse(format!(
                            "The recipe lists {} {}, but the package it names holds version {}.",
                            c.name, c.version, m.version
                        )))
                    }
                    Some(_) => {}
                }
            }
            if let Some(extra) = found.iter().find(|m| {
                !components
                    .iter()
                    .any(|c| c.unique_id.eq_ignore_ascii_case(&m.unique_id))
            }) {
                return Err(refuse(format!(
                    "The package for {} also holds {} {}, which the recipe does not list.",
                    components[0].name, extra.unique_id, extra.version
                )));
            }
        }
        Ok(())
    }

    /// Builds a new profile holding exactly the mods a restore point (or,
    /// with `KNOWN_GOOD_POINT`, the last working setup) recorded: the same
    /// versions and enabled state, from the stored packages. Only packages
    /// still stored intact are used; a mod whose exact package is gone is
    /// reported as not recreated and nothing else is put in its place. The
    /// source profile is not changed.
    pub fn recreate_from_point(
        &self,
        source_id: &ProfileId,
        point_id: &str,
        new_name: &str,
    ) -> AppResult<BundleImportDto> {
        let preferences = self.copy_journal.as_ref().ok_or_else(|| {
            AppError::internal("Restore points cannot be read here", "no preferences store")
        })?;
        let source = self
            .profile_repo
            .get_profile(source_id)?
            .ok_or_else(|| AppError::validation("PROFILE_NOT_FOUND", "Profile not found"))?;
        let (label, mods, point_settings) = if point_id
            == crate::services::restore_points::KNOWN_GOOD_POINT
        {
            let record = crate::services::known_good::stored_known_good(&**preferences, source_id)?
                .ok_or_else(|| {
                    AppError::validation(
                        "KNOWN_GOOD_NOT_RECORDED",
                        "This profile has not been seen working yet",
                    )
                })?;
            (
                "the last working setup".to_string(),
                record.mods,
                Vec::new(),
            )
        } else {
            let point = crate::services::restore_points::stored_points(&**preferences, source_id)?
                .into_iter()
                .find(|p| p.id == point_id)
                .ok_or_else(|| {
                    AppError::validation("RESTORE_POINT_NOT_FOUND", "That restore point is gone")
                })?;
            (point.label, point.mods, point.settings)
        };
        let components = mods
            .iter()
            .map(|m| RecipeComponent {
                unique_id: m.unique_id.clone(),
                name: m.name.clone(),
                author: String::new(),
                version: m.version.clone(),
                enabled: m.enabled,
                artifact_hash: m.artifact_hash.clone(),
                optional: false,
                version_rule: None,
                group: None,
                manual: None,
                client_only: false,
                note: None,
                settings: Vec::new(),
                update_keys: Vec::new(),
                source_url: None,
                requires: Vec::new(),
                locally_modified: false,
                platforms: Vec::new(),
            })
            .collect();
        let recipe = ProfileRecipe::new(
            source.name.clone(),
            chrono::Utc::now().to_rfc3339(),
            RecipeGame::default(),
            components,
        );
        let mut packages = Vec::new();
        let mut seen = HashSet::new();
        for m in &mods {
            let key = m.artifact_hash.to_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            let Ok(hash) = ArtifactHash::parse(key) else {
                continue;
            };
            if self.packages.has_artifact(&hash) && self.packages.verify_artifact(&hash)? {
                packages.push((hash.clone(), self.packages.get_artifact_path(&hash)?));
            }
        }
        let profile = self.profiles.create_profile_copy(
            &source,
            new_name,
            Some(&format!("Recreated from \"{label}\" of {}", source.name)),
        )?;
        let profile_id = ProfileId::from_str(&profile.id)
            .map_err(|e| AppError::internal("Profile id was not readable", e.to_string()))?;
        let (installed, disabled, failures) = self.materialise(
            &profile_id,
            &recipe,
            &packages,
            "Its exact package is no longer stored intact, so nothing was put in its place",
            &format!("Recreating \"{label}\" as \"{}\"", profile.name),
        )?;
        // The settings the point kept, from the source profile's backups,
        // into the mods just installed. A backup that is gone is skipped.
        let mut settings_applied = Vec::new();
        let mut failures = failures;
        if let (Some(files), Some(backups)) = (&self.files, &self.config_backups) {
            let folders = self.folders(&profile_id)?;
            for entry in &point_settings {
                let Some(folder) = folders.get(&entry.unique_id.to_lowercase()) else {
                    continue;
                };
                match backups
                    .load(source_id, &entry.backup_id)
                    .and_then(|saved| files.write_files(&profile_id, folder, &saved))
                {
                    Ok(()) => settings_applied.push(entry.name.clone()),
                    Err(e) => failures.push(crate::api::dto::BundleFailureDto {
                        name: entry.name.clone(),
                        reason: format!("Its settings were not put back: {}", e.summary),
                    }),
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
            declined_optional: Vec::new(),
            reference_attached: false,
            not_for_this_computer: Vec::new(),
            installed_with_package: Vec::new(),
        })
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
            &format!("Duplicating into \"{profile_name}\""),
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
            declined_optional: Vec::new(),
            reference_attached: false,
            not_for_this_computer: Vec::new(),
            installed_with_package: Vec::new(),
        })
    }

    /// Compares the settings files of the mods two profiles share, by
    /// content. Mods with no settings in either profile are left out, and
    /// file contents are never returned.
    pub fn compare_settings(
        &self,
        first: &ProfileId,
        second: &ProfileId,
    ) -> AppResult<Vec<SettingsComparisonDto>> {
        let files = self.files.as_ref().ok_or_else(|| {
            AppError::internal("Settings cannot be read here", "no deployed-files port")
        })?;
        let names = |profile: &ProfileId| -> AppResult<HashMap<String, String>> {
            let mut out = HashMap::new();
            for pc in self.deployment_repo.list_profile_components(profile)? {
                if let Some(component) = self
                    .package_repo
                    .get_package_component(&pc.package_component_id)?
                {
                    out.insert(component.unique_id.as_str().to_lowercase(), component.name);
                }
            }
            Ok(out)
        };
        let folders_a = self.folders(first)?;
        let folders_b = self.folders(second)?;
        let names_a = names(first)?;
        let mut out = Vec::new();
        for (unique_id, folder_a) in &folders_a {
            let Some(folder_b) = folders_b.get(unique_id) else {
                continue;
            };
            let a: BTreeMap<String, Vec<u8>> =
                files.read_configs(first, folder_a)?.into_iter().collect();
            let b: BTreeMap<String, Vec<u8>> =
                files.read_configs(second, folder_b)?.into_iter().collect();
            if a.is_empty() && b.is_empty() {
                continue;
            }
            let state = match (a.is_empty(), b.is_empty()) {
                (false, true) => "only_first",
                (true, false) => "only_second",
                _ => "",
            };
            let mut differing: Vec<String> = a
                .keys()
                .chain(b.keys())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|path| a.get(*path) != b.get(*path))
                .cloned()
                .collect();
            differing.sort();
            let state = if !state.is_empty() {
                state
            } else if differing.is_empty() {
                "same"
            } else {
                "different"
            };
            out.push(SettingsComparisonDto {
                unique_id: unique_id.clone(),
                name: names_a
                    .get(unique_id)
                    .cloned()
                    .unwrap_or_else(|| unique_id.clone()),
                state: state.to_string(),
                files: differing,
            });
        }
        out.sort_by_key(|a| a.name.to_lowercase());
        Ok(out)
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
            // Only mods whose package is still stored can be copied, so only
            // those are expected; the others are reported when finishing.
            let expected_mods = pending
                .recipe
                .components
                .iter()
                .filter(|c| {
                    ArtifactHash::parse(c.artifact_hash.to_lowercase())
                        .is_ok_and(|hash| self.packages.has_artifact(&hash))
                })
                .count();
            out.push(UnfinishedCopyDto {
                profile_id: profile.id.to_string(),
                profile_name: profile.name,
                source_name: pending.source_name,
                expected_mods,
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
        self.import_bundle_choosing(path, game_id, profile_name, None)
    }

    /// Imports a bundle, installing its optional mods only when they are in
    /// `include_optional` (by UniqueID). `None` installs every mod, optional
    /// or not.
    pub fn import_bundle_choosing(
        &self,
        path: &Path,
        game_id: &GameInstallationId,
        profile_name: &str,
        include_optional: Option<&[String]>,
    ) -> AppResult<BundleImportDto> {
        self.import_bundle_with(path, game_id, profile_name, include_optional, None, None)
    }

    /// As [`Self::import_bundle_choosing`], applying the bundled settings
    /// only for the mods in `include_settings` (by UniqueID). `None` applies
    /// all of them.
    pub fn import_bundle_with(
        &self,
        path: &Path,
        game_id: &GameInstallationId,
        profile_name: &str,
        include_optional: Option<&[String]>,
        include_settings: Option<&[String]>,
        expected_digest: Option<&str>,
    ) -> AppResult<BundleImportDto> {
        let work = self
            .work_dir
            .join(format!("import-{}", uuid::Uuid::new_v4()));
        self.set_progress(None);
        let result = self.import_from(
            path,
            game_id,
            profile_name,
            include_optional,
            include_settings,
            expected_digest,
            &work,
        );
        self.set_progress(None);
        // The extracted copies are scratch space either way.
        self.archive.discard_scratch(&work);
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn import_from(
        &self,
        path: &Path,
        game_id: &GameInstallationId,
        profile_name: &str,
        include_optional: Option<&[String]>,
        include_settings: Option<&[String]>,
        expected_digest: Option<&str>,
        work: &Path,
    ) -> AppResult<BundleImportDto> {
        let contents = self.archive.read_bundle(path, work)?;
        // The preview is what the user agreed to: a file that changed since
        // it was read (re-exported, synced or still downloading) is not it.
        if let Some(expected) = expected_digest {
            if !expected.eq_ignore_ascii_case(&contents.digest) {
                return Err(AppError::validation(
                    "BUNDLE_CHANGED",
                    "The bundle file has changed since it was read, so nothing was imported. Read it again to see what it holds now.",
                ));
            }
        }
        let mut recipe = Self::parse_recipe(&contents.recipe_json)?;
        // Checked before anything is created: each package must be the mod
        // the recipe, and so the preview, said it was.
        self.check_identities(&recipe, &contents.packages)?;
        // Mods for other operating systems are never installed here, nor is
        // a package that holds one of them alongside others.
        let not_for_this_computer: Vec<String> = recipe
            .components
            .iter()
            .filter(|c| self.not_for_here(c))
            .map(|c| c.name.clone())
            .collect();
        let excluded_hashes = self.excluded_packages(&recipe);
        let mut shared_package_failures = Vec::new();
        recipe.components.retain(|c| {
            if self.not_for_here(c) {
                return false;
            }
            if excluded_hashes.contains(&c.artifact_hash.to_lowercase()) {
                shared_package_failures.push(BundleFailureDto {
                    name: c.name.clone(),
                    reason: "It comes in one package with a mod that is not for this computer, so it was not installed.".to_string(),
                });
                return false;
            }
            true
        });
        // Optional mods are installed only when chosen. One that comes in a
        // package with a mod that is installed cannot be left out of it (a
        // package is one folder, enabled or disabled as a whole), so it comes
        // too, as the preview says, and is reported as such.
        let mut declined_optional = Vec::new();
        let mut declined_ids: BTreeMap<String, String> = BTreeMap::new();
        let mut installed_with_package = Vec::new();
        if let Some(chosen) = include_optional {
            let wanted = |c: &RecipeComponent| {
                !c.optional
                    || chosen
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(&c.unique_id))
            };
            let kept_packages: HashSet<String> = recipe
                .components
                .iter()
                .filter(|c| wanted(c) && !c.artifact_hash.is_empty())
                .map(|c| c.artifact_hash.to_lowercase())
                .collect();
            recipe.components.retain(|c| {
                if wanted(c) {
                    return true;
                }
                if kept_packages.contains(&c.artifact_hash.to_lowercase()) {
                    installed_with_package.push(c.name.clone());
                    return true;
                }
                declined_optional.push(c.name.clone());
                declined_ids.insert(c.unique_id.to_lowercase(), c.name.clone());
                false
            });
        }

        let profile =
            self.profiles
                .create_profile(game_id, profile_name, Some("Imported from a bundle"))?;
        let profile_id = ProfileId::from_str(&profile.id)
            .map_err(|e| AppError::internal("Profile id was not readable", e.to_string()))?;

        let packages: Vec<(ArtifactHash, PathBuf)> = contents
            .packages
            .iter()
            .filter(|p| !excluded_hashes.contains(&p.hash.as_str().to_lowercase()))
            .map(|p| (p.hash.clone(), p.path.clone()))
            .collect();
        let (installed, disabled, mut failures) = self.materialise(
            &profile_id,
            &recipe,
            &packages,
            "The bundle does not include this mod's package",
            &format!("Importing a bundle into \"{}\"", profile.name),
        )?;
        failures.extend(shared_package_failures);
        if !declined_optional.is_empty() {
            for failure in &mut failures {
                let reason = failure.reason.to_lowercase();
                let named: Vec<&str> = declined_ids
                    .iter()
                    .filter(|(id, _)| reason.contains(id.as_str()))
                    .map(|(_, name)| name.as_str())
                    .collect();
                if !named.is_empty() {
                    failure.reason = sentences([
                        failure.reason.as_str(),
                        &format!(
                            "It needs {}, which the bundle offered as optional and you left out.",
                            named.join(", ")
                        ),
                    ]);
                } else if reason.contains("needs a mod that is not installed") {
                    failure.reason = sentences([
                        failure.reason.as_str(),
                        &format!(
                            "It may need one of the optional mods you left out ({}).",
                            declined_optional.join(", ")
                        ),
                    ]);
                }
            }
        }

        // The full mod list stays as the profile's reference; optional mods
        // the recipient left out are accepted differences, not open ones.
        let mut reference_attached = false;
        if let Some(references) = &self.import_references {
            if references
                .attach(&profile_id, &contents.recipe_json)
                .is_ok()
            {
                reference_attached = true;
                if let Ok(full) = Self::parse_recipe(&contents.recipe_json) {
                    // Keys as the group reference builds them (recipe.ts,
                    // differenceKey): a missing mod's key names its package.
                    for component in full
                        .components
                        .iter()
                        .filter(|c| declined_ids.contains_key(&c.unique_id.to_lowercase()))
                    {
                        let _ = references.set_accepted(
                            &profile_id,
                            &format!(
                                "missing:{}:{}::{}:",
                                component.unique_id,
                                component.version,
                                component.artifact_hash.to_lowercase()
                            ),
                            true,
                        );
                    }
                }
            }
        }

        let not_here_ids: HashSet<String> = Self::parse_recipe(&contents.recipe_json)?
            .components
            .iter()
            .filter(|c| self.not_for_here(c))
            .map(|c| c.unique_id.to_lowercase())
            .collect();
        for (unique_id, reason) in &contents.skipped_settings {
            let id = unique_id.to_lowercase();
            if declined_ids.contains_key(&id) || not_here_ids.contains(&id) {
                continue;
            }
            failures.push(BundleFailureDto {
                name: unique_id.clone(),
                reason: format!("Its settings were not applied: {reason}"),
            });
        }

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
                let not_here = &not_here_ids;
                for (unique_id, entries) in by_mod {
                    // A declined optional mod's settings are not a failure,
                    // nor are those of a mod not for this computer, nor
                    // settings the recipient chose not to use.
                    if declined_ids.contains_key(&unique_id) || not_here.contains(&unique_id) {
                        continue;
                    }
                    if let Some(chosen) = include_settings {
                        if !chosen.iter().any(|id| id.eq_ignore_ascii_case(&unique_id)) {
                            continue;
                        }
                    }
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
            declined_optional,
            reference_attached,
            not_for_this_computer,
            installed_with_package,
        })
    }
}
