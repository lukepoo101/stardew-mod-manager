//! Named restore points for a profile's mods.
//!
//! A restore point records every mod in the profile with its exact version,
//! package checksum and enabled state. Restoring works out a plan first and
//! refuses outright when a package the point needs is no longer kept intact,
//! rather than restoring part of it. Before changing anything it saves the
//! current state as a new restore point, so a restore can itself be undone.
//! Every change goes through the normal journaled remove, install and
//! enable/disable operations.

use crate::api::dto::{FrozenModDto, RestorePlanDto, RestorePointDto, RestoreResultDto};
use crate::error::{AppError, AppResult};
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

pub const MAX_RESTORE_POINTS: usize = 20;
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

/// Saves a profile's current mods as a restore point and reads it back. Used
/// directly and automatically before changes that replace mods.
pub fn record_point(
    preferences: &dyn PreferencesRepository,
    deployment_repo: &dyn DeploymentRepository,
    package_repo: &dyn PackageCatalogRepository,
    profile_id: &ProfileId,
    label: &str,
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
    };
    let mut points: Vec<RestorePointDto> = preferences
        .get_preference(&key(profile_id))?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    points.insert(0, point.clone());
    points.truncate(MAX_RESTORE_POINTS);
    let json = serde_json::to_string(&points)
        .map_err(|e| AppError::internal("Could not save restore points", e.to_string()))?;
    preferences.set_preference(&key(profile_id), &json)?;
    // Verify it is there before anything relies on it.
    let stored: Vec<RestorePointDto> = preferences
        .get_preference(&key(profile_id))?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    if !stored.iter().any(|p| p.id == point.id) {
        return Err(AppError::storage(
            "RESTORE_POINT_NOT_SAVED",
            "The restore point could not be saved, so nothing was changed",
        ));
    }
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
        }
    }

    pub fn list(&self, profile_id: &ProfileId) -> AppResult<Vec<RestorePointDto>> {
        stored_points(&*self.preferences, profile_id)
    }

    fn save_all(&self, profile_id: &ProfileId, points: &[RestorePointDto]) -> AppResult<()> {
        let json = serde_json::to_string(points)
            .map_err(|e| AppError::internal("Could not save restore points", e.to_string()))?;
        self.preferences.set_preference(&key(profile_id), &json)
    }

    /// Saves the profile as it is now. The oldest points beyond the limit go.
    pub fn create(&self, profile_id: &ProfileId, label: &str) -> AppResult<RestorePointDto> {
        record_point(
            &*self.preferences,
            &*self.deployment_repo,
            &*self.package_repo,
            profile_id,
            label,
        )
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
        Ok(self.work(profile_id, &point)?.plan)
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
        let undo = self.create(profile_id, &format!("Before restoring \"{}\"", point.label))?;
        let mut done = Vec::new();
        let mut failed = Vec::new();

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
                Ok(())
            };
            match step() {
                Ok(()) => done.push("Removed a mod the restore point does not have".to_string()),
                Err(e) => failed.push(format!("Removing a mod: {}", e.summary)),
            }
        }
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
        for hash in &work.install {
            let step = || -> AppResult<()> {
                let hash = ArtifactHash::parse(hash.clone())
                    .map_err(|_| AppError::internal("Package checksum", hash.clone()))?;
                let path = self.packages.get_artifact_path(&hash)?;
                self.reinstall.install(profile_id, &path).map(|_| ())
            };
            match step() {
                Ok(()) => done.push("Installed a mod from the restore point".to_string()),
                Err(e) => failed.push(format!("Installing a mod: {}", e.summary)),
            }
        }

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

        Ok(RestoreResultDto {
            undo_point_id: undo.id,
            done,
            failed,
        })
    }
}
