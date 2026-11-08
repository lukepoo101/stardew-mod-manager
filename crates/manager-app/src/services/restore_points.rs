//! Named restore points for a profile's mods.
//!
//! A restore point records every mod in the profile with its exact version,
//! package checksum and enabled state. Restoring works out a plan first and
//! refuses outright when a package the point needs is no longer kept intact,
//! rather than restoring part of it. Before changing anything it saves the
//! current state as a new restore point, so a restore can itself be undone.
//! Every change goes through the normal journaled remove, install and
//! enable/disable operations.

use crate::api::dto::{
    FrozenModDto, PointSettingsDto, RestorePlanDto, RestorePointDto, RestoreResultDto,
};
use crate::error::{AppError, AppResult};
use crate::ports::config_backups::ConfigBackupsPort;
use crate::ports::deployed_files::DeployedFilesPort;
use crate::ports::repositories::{
    DeploymentRepository, PackageCatalogRepository, PreferencesRepository,
};
use crate::services::freeze::snapshot_mods;
use crate::services::{
    ModsService, OperationsService, PackagesService, ReinstallService, ToggleService,
};
use chrono::Utc;
use manager_core::ids::{ArtifactHash, OperationId, ProfileComponentId, ProfileId};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::str::FromStr;
use std::sync::Arc;

/// Points the user saves and names. The oldest of them goes when another is
/// saved beyond this.
pub const MAX_RESTORE_POINTS: usize = 20;
/// Points saved automatically before a change (freezing, restoring,
/// replacing a version). They have their own limit, so they never push out
/// a point the user saved.
pub const MAX_AUTOMATIC_POINTS: usize = 20;
/// The point id that means "the profile's last working setup": the
/// known-good record, restored like any other point (versions included).
pub const KNOWN_GOOD_POINT: &str = "known-good";
pub const MAX_LABEL_CHARS: usize = 80;

fn key(profile_id: &ProfileId) -> String {
    format!("restore_points:{profile_id}")
}

/// The restore points saved for a profile, newest first.
pub fn stored_points(
    preferences: &dyn PreferencesRepository,
    profile_id: &ProfileId,
) -> AppResult<Vec<RestorePointDto>> {
    Ok(preferences
        .get_preference(&key(profile_id))?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

/// Keeps the newest points of each kind within its own limit. `points` is
/// newest first.
fn prune(points: &mut Vec<RestorePointDto>) {
    let (mut named, mut automatic) = (0, 0);
    points.retain(|point| {
        let (count, limit) = if point.automatic {
            (&mut automatic, MAX_AUTOMATIC_POINTS)
        } else {
            (&mut named, MAX_RESTORE_POINTS)
        };
        *count += 1;
        *count <= limit
    });
}

/// Adds a point as the newest, within the limits, and checks it was stored.
fn insert_point(
    preferences: &dyn PreferencesRepository,
    profile_id: &ProfileId,
    point: &RestorePointDto,
) -> AppResult<()> {
    let mut points = stored_points(preferences, profile_id)?;
    points.insert(0, point.clone());
    prune(&mut points);
    let json = serde_json::to_string(&points)
        .map_err(|e| AppError::internal("Could not save restore points", e.to_string()))?;
    preferences.set_preference(&key(profile_id), &json)?;
    // Verify it is there before anything relies on it.
    if !stored_points(preferences, profile_id)?
        .iter()
        .any(|p| p.id == point.id)
    {
        return Err(AppError::storage(
            "RESTORE_POINT_NOT_SAVED",
            "The restore point could not be saved, so nothing was changed",
        ));
    }
    Ok(())
}

/// Notes an operation made right after an automatic restore point, so the
/// point says which change it was taken before.
pub fn note_operation(
    preferences: &dyn PreferencesRepository,
    profile_id: &ProfileId,
    point_id: &str,
    operation_id: &OperationId,
) -> AppResult<()> {
    let mut points = stored_points(preferences, profile_id)?;
    let Some(point) = points.iter_mut().find(|p| p.id == point_id) else {
        return Ok(());
    };
    let id = operation_id.to_string();
    if !point.operations.contains(&id) {
        point.operations.push(id);
    }
    let json = serde_json::to_string(&points)
        .map_err(|e| AppError::internal("Could not save restore points", e.to_string()))?;
    preferences.set_preference(&key(profile_id), &json)
}

/// Saves a profile's current mods as an automatic restore point, before a
/// change that replaces mods, and reads it back.
pub fn record_point(
    preferences: &dyn PreferencesRepository,
    deployment_repo: &dyn DeploymentRepository,
    package_repo: &dyn PackageCatalogRepository,
    profile_id: &ProfileId,
    label: &str,
) -> AppResult<RestorePointDto> {
    record_point_with_source(
        preferences,
        deployment_repo,
        package_repo,
        profile_id,
        label,
        None,
    )
}

/// As `record_point`, saying where the change it was taken before came
/// from (a downloaded update's GitHub release).
pub fn record_point_with_source(
    preferences: &dyn PreferencesRepository,
    deployment_repo: &dyn DeploymentRepository,
    package_repo: &dyn PackageCatalogRepository,
    profile_id: &ProfileId,
    label: &str,
    source: Option<String>,
) -> AppResult<RestorePointDto> {
    let mut point = record_unsaved(deployment_repo, package_repo, profile_id, label, true)?;
    point.source = source;
    insert_point(preferences, profile_id, &point)?;
    Ok(point)
}

fn record(
    preferences: &dyn PreferencesRepository,
    deployment_repo: &dyn DeploymentRepository,
    package_repo: &dyn PackageCatalogRepository,
    profile_id: &ProfileId,
    label: &str,
    automatic: bool,
) -> AppResult<RestorePointDto> {
    let point = record_unsaved(deployment_repo, package_repo, profile_id, label, automatic)?;
    insert_point(preferences, profile_id, &point)?;
    Ok(point)
}

fn record_unsaved(
    deployment_repo: &dyn DeploymentRepository,
    package_repo: &dyn PackageCatalogRepository,
    profile_id: &ProfileId,
    label: &str,
    automatic: bool,
) -> AppResult<RestorePointDto> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
        return Err(AppError::validation(
            "RESTORE_POINT_LABEL",
            format!("Give the restore point a name of 1 to {MAX_LABEL_CHARS} characters"),
        ));
    }
    let point = RestorePointDto {
        id: uuid::Uuid::new_v4().to_string(),
        label: label.to_string(),
        created_at: Utc::now().to_rfc3339(),
        mods: snapshot_mods(deployment_repo, package_repo, profile_id)?,
        operations: Vec::new(),
        settings: Vec::new(),
        game_version: None,
        smapi_version: None,
        automatic,
        source: None,
    };
    Ok(point)
}

/// Keeps a given list of mods as an automatic restore point, for a setup
/// that is about to stop being recorded elsewhere (such as a replaced
/// known-good record).
pub fn keep_as_point(
    preferences: &dyn PreferencesRepository,
    profile_id: &ProfileId,
    label: &str,
    created_at: &str,
    mods: Vec<FrozenModDto>,
) -> AppResult<RestorePointDto> {
    let point = RestorePointDto {
        id: uuid::Uuid::new_v4().to_string(),
        label: label.chars().take(MAX_LABEL_CHARS).collect(),
        created_at: created_at.to_string(),
        mods,
        operations: Vec::new(),
        settings: Vec::new(),
        game_version: None,
        smapi_version: None,
        automatic: true,
        source: None,
    };
    insert_point(preferences, profile_id, &point)?;
    Ok(point)
}

pub struct RestorePoints {
    preferences: Arc<dyn PreferencesRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    package_repo: Arc<dyn PackageCatalogRepository>,
    packages: Arc<PackagesService>,
    mods: Arc<ModsService>,
    operations: Arc<OperationsService>,
    toggle: Arc<ToggleService>,
    reinstall: Arc<ReinstallService>,
    /// Where mods' settings are read and written, and kept as backups. Without
    /// it, points record mods only.
    settings: Option<(Arc<dyn DeployedFilesPort>, Arc<dyn ConfigBackupsPort>)>,
    /// Journals a restore as one change set, so an interrupted one is known.
    change_sets: Option<Arc<crate::services::ChangeSets>>,
}

/// The plan, with the ids needed to carry it out.
struct Work {
    plan: RestorePlanDto,
    /// One component of each deployment to remove.
    remove: Vec<ProfileComponentId>,
    /// Packages whose version replaces an installed one.
    replace: BTreeSet<String>,
    /// Packages to install that nothing installed conflicts with.
    install: BTreeSet<String>,
    /// UniqueID (lowercase) -> enabled state wanted.
    enabled: HashMap<String, bool>,
}

impl RestorePoints {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        preferences: Arc<dyn PreferencesRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        package_repo: Arc<dyn PackageCatalogRepository>,
        packages: Arc<PackagesService>,
        mods: Arc<ModsService>,
        operations: Arc<OperationsService>,
        toggle: Arc<ToggleService>,
        reinstall: Arc<ReinstallService>,
    ) -> Self {
        Self {
            preferences,
            deployment_repo,
            package_repo,
            packages,
            mods,
            operations,
            toggle,
            reinstall,
            settings: None,
            change_sets: None,
        }
    }

    pub fn with_change_sets(mut self, change_sets: Arc<crate::services::ChangeSets>) -> Self {
        self.change_sets = Some(change_sets);
        self
    }

    /// Lets points made by hand keep mods' settings, and restores put them back.
    pub fn with_settings(
        mut self,
        files: Arc<dyn DeployedFilesPort>,
        backups: Arc<dyn ConfigBackupsPort>,
    ) -> Self {
        self.settings = Some((files, backups));
        self
    }

    /// Lower-case UniqueID -> (folder, name) of the profile's mods.
    fn folders(&self, profile_id: &ProfileId) -> AppResult<HashMap<String, (String, String)>> {
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
                    (deployment.root_relative_path, component.name),
                );
            }
        }
        Ok(out)
    }

    /// Backs up every mod's settings files for a point.
    fn capture_settings(&self, profile_id: &ProfileId) -> AppResult<Vec<PointSettingsDto>> {
        let Some((files, backups)) = &self.settings else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut folders: Vec<_> = self.folders(profile_id)?.into_iter().collect();
        folders.sort();
        for (unique_id, (folder, name)) in folders {
            let found = files.read_configs(profile_id, &folder)?;
            if found.is_empty() {
                continue;
            }
            let backup = backups.save(profile_id, &unique_id, &found)?;
            out.push(PointSettingsDto {
                unique_id,
                name,
                backup_id: backup.id,
                files: backup.files,
            });
        }
        Ok(out)
    }

    /// Whether a point's settings backup is still kept.
    fn settings_kept(&self, profile_id: &ProfileId, entry: &PointSettingsDto) -> bool {
        let Some((_, backups)) = &self.settings else {
            return false;
        };
        backups
            .list(profile_id, &entry.unique_id)
            .map(|list| list.iter().any(|b| b.id == entry.backup_id))
            .unwrap_or(false)
    }

    /// Whether putting back a point's saved settings would change anything:
    /// the mod's folder lacks one of the saved files or holds it with other
    /// content. A mod that is not installed now always needs them.
    fn settings_differ(
        &self,
        profile_id: &ProfileId,
        folders: &HashMap<String, (String, String)>,
        entry: &PointSettingsDto,
    ) -> bool {
        let Some((files, backups)) = &self.settings else {
            return false;
        };
        let Some((folder, _)) = folders.get(&entry.unique_id.to_lowercase()) else {
            return true;
        };
        let (Ok(saved), Ok(current)) = (
            backups.load(profile_id, &entry.backup_id),
            files.read_configs(profile_id, folder),
        ) else {
            return true;
        };
        let current: HashMap<String, Vec<u8>> = current.into_iter().collect();
        saved
            .iter()
            .any(|(path, bytes)| current.get(path) != Some(bytes))
    }

    pub fn list(&self, profile_id: &ProfileId) -> AppResult<Vec<RestorePointDto>> {
        stored_points(&*self.preferences, profile_id)
    }

    fn save_all(&self, profile_id: &ProfileId, points: &[RestorePointDto]) -> AppResult<()> {
        let json = serde_json::to_string(points)
            .map_err(|e| AppError::internal("Could not save restore points", e.to_string()))?;
        self.preferences.set_preference(&key(profile_id), &json)
    }

    /// Saves the profile as it is now, as a point the user named. The oldest
    /// named points beyond the limit go; automatic points never push them out.
    pub fn create(&self, profile_id: &ProfileId, label: &str) -> AppResult<RestorePointDto> {
        self.create_with(profile_id, label, false)
    }

    /// Saves the profile as it is now, with its settings. `automatic` is for
    /// points the app saves before a change (freezing, restoring).
    pub fn create_with(
        &self,
        profile_id: &ProfileId,
        label: &str,
        automatic: bool,
    ) -> AppResult<RestorePointDto> {
        let mut point = record(
            &*self.preferences,
            &*self.deployment_repo,
            &*self.package_repo,
            profile_id,
            label,
            automatic,
        )?;
        let settings = self.capture_settings(profile_id)?;
        if !settings.is_empty() {
            let mut points = self.list(profile_id)?;
            if let Some(stored) = points.iter_mut().find(|p| p.id == point.id) {
                stored.settings = settings.clone();
            }
            self.save_all(profile_id, &points)?;
            point.settings = settings;
        }
        Ok(point)
    }

    /// Records the runtime last observed on a point, as context.
    pub fn note_runtime(
        &self,
        profile_id: &ProfileId,
        point_id: &str,
        game_version: Option<String>,
        smapi_version: Option<String>,
    ) -> AppResult<RestorePointDto> {
        let mut points = self.list(profile_id)?;
        let point = points
            .iter_mut()
            .find(|p| p.id == point_id)
            .ok_or_else(|| {
                AppError::validation("RESTORE_POINT_NOT_FOUND", "That restore point is gone")
            })?;
        point.game_version = game_version;
        point.smapi_version = smapi_version;
        let updated = point.clone();
        self.save_all(profile_id, &points)?;
        Ok(updated)
    }

    pub fn delete(&self, profile_id: &ProfileId, point_id: &str) -> AppResult<()> {
        let mut points = self.list(profile_id)?;
        points.retain(|p| p.id != point_id);
        self.save_all(profile_id, &points)
    }

    fn find(&self, profile_id: &ProfileId, point_id: &str) -> AppResult<RestorePointDto> {
        if point_id == KNOWN_GOOD_POINT {
            let record =
                crate::services::known_good::stored_known_good(&*self.preferences, profile_id)?
                    .ok_or_else(|| {
                        AppError::validation(
                            "KNOWN_GOOD_NOT_RECORDED",
                            "This profile has not been seen working yet",
                        )
                    })?;
            return Ok(RestorePointDto {
                id: KNOWN_GOOD_POINT.to_string(),
                label: "the last working setup".to_string(),
                created_at: record.recorded_at,
                mods: record.mods,
                operations: Vec::new(),
                settings: Vec::new(),
                game_version: record.game_version,
                smapi_version: record.smapi_version,
                automatic: true,
                source: None,
            });
        }
        self.list(profile_id)?
            .into_iter()
            .find(|p| p.id == point_id)
            .ok_or_else(|| {
                AppError::validation("RESTORE_POINT_NOT_FOUND", "That restore point is gone")
            })
    }

    fn work(&self, profile_id: &ProfileId, point: &RestorePointDto) -> AppResult<Work> {
        let lower = |s: &str| s.to_lowercase();
        let target: BTreeMap<String, &FrozenModDto> = point
            .mods
            .iter()
            .map(|m| (lower(&m.unique_id), m))
            .collect();

        // What is installed now, with a component id per UniqueID.
        let mut current: BTreeMap<String, (FrozenModDto, ProfileComponentId, String)> =
            BTreeMap::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            let Some(component) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            let deployment_hash = self
                .deployment_repo
                .get_deployment(&pc.deployment_id)?
                .map(|d| d.artifact_hash.as_str().to_string())
                .unwrap_or_else(|| component.artifact_hash.to_string());
            current.insert(
                lower(component.unique_id.as_str()),
                (
                    FrozenModDto {
                        unique_id: component.unique_id.to_string(),
                        name: component.name,
                        version: component.version,
                        artifact_hash: deployment_hash,
                        enabled: pc.enabled,
                    },
                    pc.id,
                    pc.deployment_id.to_string(),
                ),
            );
        }

        let mut plan = RestorePlanDto {
            point_id: point.id.clone(),
            available: true,
            unavailable: Vec::new(),
            remove: Vec::new(),
            install: Vec::new(),
            change_version: Vec::new(),
            enable: Vec::new(),
            disable: Vec::new(),
            settings: Vec::new(),
            settings_unavailable: Vec::new(),
        };
        let mut remove = Vec::new();
        let mut removed_deployments = BTreeSet::new();
        let mut replace = BTreeSet::new();
        let mut install = BTreeSet::new();
        let mut enabled = HashMap::new();

        for (id, (now, component_id, deployment)) in &current {
            if !target.contains_key(id) {
                plan.remove.push(format!("{} {}", now.name, now.version));
                if removed_deployments.insert(deployment.clone()) {
                    remove.push(*component_id);
                }
            }
        }
        for (id, wanted) in &target {
            enabled.insert(id.clone(), wanted.enabled);
            let hash = wanted.artifact_hash.to_lowercase();
            match current.get(id) {
                Some((now, _, _)) if now.artifact_hash.eq_ignore_ascii_case(&hash) => {
                    if now.enabled != wanted.enabled {
                        if wanted.enabled {
                            plan.enable.push(wanted.name.clone());
                        } else {
                            plan.disable.push(wanted.name.clone());
                        }
                    }
                }
                Some((now, _, _)) => {
                    plan.change_version.push(format!(
                        "{} {} → {}",
                        wanted.name, now.version, wanted.version
                    ));
                    replace.insert(hash);
                }
                None => {
                    plan.install
                        .push(format!("{} {}", wanted.name, wanted.version));
                    install.insert(hash);
                }
            }
        }

        // Every package the plan needs must be here and intact, or nothing runs.
        for hash in replace.iter().chain(install.iter()) {
            let ok = ArtifactHash::parse(hash.clone())
                .ok()
                .filter(|h| self.packages.has_artifact(h))
                .map(|h| self.packages.verify_artifact(&h))
                .transpose()?
                .unwrap_or(false);
            if !ok {
                plan.available = false;
                let names: Vec<String> = point
                    .mods
                    .iter()
                    .filter(|m| m.artifact_hash.eq_ignore_ascii_case(hash))
                    .map(|m| format!("{} {}", m.name, m.version))
                    .collect();
                plan.unavailable.push(format!(
                    "{}: its package is no longer kept intact",
                    names.join(", ")
                ));
            }
        }
        // A package that replaces one mod installs its others too.
        install.retain(|h| !replace.contains(h));
        // A frozen profile refuses installs and removals, so a point that
        // needs them is refused whole rather than restored in part.
        if !(remove.is_empty() && replace.is_empty() && install.is_empty()) {
            if let Some(reason) =
                crate::services::freeze::frozen_reason(&*self.preferences, profile_id)?
            {
                plan.available = false;
                plan.unavailable.push(format!(
                    "This profile is frozen{}, and this point would install, remove or change the version of mods. Unfreeze it on the Profiles page first.",
                    if reason.is_empty() {
                        String::new()
                    } else {
                        format!(" ({reason})")
                    }
                ));
            }
        }
        Ok(Work {
            plan,
            remove,
            replace,
            install,
            enabled,
        })
    }

    pub fn plan(&self, profile_id: &ProfileId, point_id: &str) -> AppResult<RestorePlanDto> {
        let point = self.find(profile_id, point_id)?;
        let mut plan = self.work(profile_id, &point)?.plan;
        let folders = self.folders(profile_id)?;
        for entry in &point.settings {
            if !self.settings_kept(profile_id, entry) {
                plan.settings_unavailable.push(entry.name.clone());
            } else if self.settings_differ(profile_id, &folders, entry) {
                // Settings already as saved are left out: nothing to put back.
                plan.settings.push(entry.name.clone());
            }
        }
        Ok(plan)
    }

    pub fn restore(&self, profile_id: &ProfileId, point_id: &str) -> AppResult<RestoreResultDto> {
        let point = self.find(profile_id, point_id)?;
        let work = self.work(profile_id, &point)?;
        if !work.plan.available {
            return Err(AppError::validation(
                "RESTORE_POINT_UNAVAILABLE",
                format!(
                    "This restore point cannot be restored: {}",
                    work.plan.unavailable.join("; ")
                ),
            ));
        }
        let undo = self.create_with(
            profile_id,
            &format!("Before restoring \"{}\"", point.label),
            true,
        )?;
        let change = format!("Restoring \"{}\"", point.label);
        let label = |id: &OperationId| {
            let _ =
                crate::services::operation_labels::label_operation(&*self.preferences, id, &change);
        };
        let mut done = Vec::new();
        let mut failed = Vec::new();

        // One change set over the whole restore, a part per kind of step.
        let parts = [
            "Remove mods the point does not have",
            "Change versions",
            "Install mods from the point",
            "Set enabled state",
            "Put back saved settings",
        ];
        let journal = match &self.change_sets {
            Some(sets) => Some(sets.begin(
                profile_id,
                &change,
                "restore_point",
                &point.id,
                Some(&undo.id),
                &parts.map(str::to_string),
            )?),
            None => None,
        };
        let mark = |index: u32, failed_before: usize, failed: &Vec<String>| {
            if let (Some(sets), Some(id)) = (&self.change_sets, &journal) {
                let error =
                    (failed.len() > failed_before).then(|| failed[failed_before..].join("; "));
                let _ = sets.part_done(id, index, error.as_deref());
            }
        };

        let before = failed.len();
        for component in &work.remove {
            let step = || -> AppResult<()> {
                // Removal works on live mods.
                if !self
                    .deployment_repo
                    .get_profile_component(component)?
                    .is_some_and(|pc| pc.enabled)
                {
                    self.toggle.set_enabled(component, true)?;
                }
                let preview = self.mods.prepare_removal(component)?;
                let id = OperationId::from_str(&preview.operation_id)
                    .map_err(|e| AppError::internal("Operation id", e.to_string()))?;
                self.operations.commit_operation(&id)?;
                label(&id);
                Ok(())
            };
            match step() {
                Ok(()) => done.push("Removed a mod the restore point does not have".to_string()),
                Err(e) => failed.push(format!("Removing a mod: {}", e.summary)),
            }
        }
        mark(0, before, &failed);
        let before = failed.len();
        for hash in &work.replace {
            match self.reinstall.replace(profile_id, hash) {
                Ok(result) => done.extend(result.replaced.iter().map(|r| {
                    format!(
                        "{} {} → {}",
                        r.name, r.installed_version, r.incoming_version
                    )
                })),
                Err(e) => failed.push(format!("Changing a version: {}", e.summary)),
            }
        }
        mark(1, before, &failed);
        let before = failed.len();
        for hash in &work.install {
            let step = || -> AppResult<()> {
                let hash = ArtifactHash::parse(hash.clone())
                    .map_err(|_| AppError::internal("Package checksum", hash.clone()))?;
                let path = self.packages.get_artifact_path(&hash)?;
                let id = self.reinstall.install(profile_id, &path)?;
                label(&id);
                Ok(())
            };
            match step() {
                Ok(()) => done.push("Installed a mod from the restore point".to_string()),
                Err(e) => failed.push(format!("Installing a mod: {}", e.summary)),
            }
        }

        mark(2, before, &failed);
        let before = failed.len();
        // Enabled state last, once every mod is back.
        let mut to_enable = Vec::new();
        let mut to_disable = Vec::new();
        for pc in self.deployment_repo.list_profile_components(profile_id)? {
            let Some(component) = self
                .package_repo
                .get_package_component(&pc.package_component_id)?
            else {
                continue;
            };
            if let Some(&wanted) = work
                .enabled
                .get(&component.unique_id.as_str().to_lowercase())
            {
                if pc.enabled != wanted {
                    if wanted {
                        to_enable.push(pc.id);
                    } else {
                        to_disable.push(pc.id);
                    }
                }
            }
        }
        for (ids, enable) in [(to_enable, true), (to_disable, false)] {
            if ids.is_empty() {
                continue;
            }
            match self.toggle.set_many_enabled(&ids, enable) {
                Ok(result) => {
                    done.extend(result.changed.iter().map(|name| {
                        format!("{} {}", if enable { "Enabled" } else { "Disabled" }, name)
                    }));
                    failed.extend(
                        result
                            .failed
                            .iter()
                            .map(|f| format!("{}: {}", f.name, f.message)),
                    );
                }
                Err(e) => failed.push(format!("Setting enabled state: {}", e.summary)),
            }
        }

        mark(3, before, &failed);
        let before = failed.len();
        // Settings last, into the restored mods' folders.
        if let Some((files, backups)) = &self.settings {
            let folders = self.folders(profile_id)?;
            for entry in &point.settings {
                if self.settings_kept(profile_id, entry)
                    && !self.settings_differ(profile_id, &folders, entry)
                {
                    continue;
                }
                let step = || -> AppResult<()> {
                    let (folder, _) =
                        folders
                            .get(&entry.unique_id.to_lowercase())
                            .ok_or_else(|| {
                                AppError::validation(
                                    "MOD_NOT_INSTALLED",
                                    "the mod is not installed",
                                )
                            })?;
                    if !self.settings_kept(profile_id, entry) {
                        return Err(AppError::validation(
                            "SETTINGS_BACKUP_GONE",
                            "its saved settings are no longer kept",
                        ));
                    }
                    let saved = backups.load(profile_id, &entry.backup_id)?;
                    files.write_files(profile_id, folder, &saved)
                };
                match step() {
                    Ok(()) => done.push(format!("Put back {}'s settings", entry.name)),
                    Err(e) => failed.push(format!("{}'s settings: {}", entry.name, e.summary)),
                }
            }
        }

        mark(4, before, &failed);
        if let (Some(sets), Some(id)) = (&self.change_sets, &journal) {
            sets.finish(id, &failed)?;
        }

        Ok(RestoreResultDto {
            undo_point_id: undo.id,
            done,
            failed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::repositories::WindowGeometryDto;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Memory(Mutex<BTreeMap<String, String>>);

    impl PreferencesRepository for Memory {
        fn get_window_geometry(&self) -> AppResult<Option<WindowGeometryDto>> {
            Ok(None)
        }
        fn save_window_geometry(&self, _: &WindowGeometryDto) -> AppResult<()> {
            Ok(())
        }
        fn get_preference(&self, key: &str) -> AppResult<Option<String>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set_preference(&self, key: &str, value: &str) -> AppResult<()> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
    }

    fn keep(preferences: &Memory, profile: &ProfileId, label: &str, automatic: bool) {
        let point = RestorePointDto {
            id: uuid::Uuid::new_v4().to_string(),
            label: label.to_string(),
            created_at: Utc::now().to_rfc3339(),
            mods: Vec::new(),
            operations: Vec::new(),
            settings: Vec::new(),
            game_version: None,
            smapi_version: None,
            automatic,
            source: None,
        };
        insert_point(preferences, profile, &point).unwrap();
    }

    #[test]
    fn automatic_points_never_push_out_named_ones() {
        let preferences = Memory::default();
        let profile = ProfileId::new();
        for n in 0..MAX_RESTORE_POINTS {
            keep(&preferences, &profile, &format!("Named {n}"), false);
        }
        for n in 0..MAX_AUTOMATIC_POINTS + 5 {
            keep(&preferences, &profile, &format!("Before change {n}"), true);
        }
        let points = stored_points(&preferences, &profile).unwrap();
        let named: Vec<_> = points.iter().filter(|p| !p.automatic).collect();
        let automatic: Vec<_> = points.iter().filter(|p| p.automatic).collect();
        assert_eq!(named.len(), MAX_RESTORE_POINTS);
        assert_eq!(named.last().unwrap().label, "Named 0");
        assert_eq!(automatic.len(), MAX_AUTOMATIC_POINTS);
        // The oldest automatic ones went; the newest are kept.
        assert_eq!(automatic[0].label, "Before change 24");
        assert_eq!(automatic.last().unwrap().label, "Before change 5");

        // One more named point replaces the oldest named one only.
        keep(&preferences, &profile, "Named new", false);
        let points = stored_points(&preferences, &profile).unwrap();
        assert!(!points.iter().any(|p| p.label == "Named 0"));
        assert!(points.iter().any(|p| p.label == "Named 1"));
        assert_eq!(
            points.iter().filter(|p| p.automatic).count(),
            MAX_AUTOMATIC_POINTS
        );
    }

    #[test]
    fn points_saved_before_kinds_were_recorded_count_as_named() {
        let point: RestorePointDto = serde_json::from_str(
            r#"{"id":"a","label":"Old","created_at":"2026-01-01T00:00:00Z","mods":[]}"#,
        )
        .unwrap();
        assert!(!point.automatic);
    }
}
