//! Reclaiming disk space from the manager's own storage.
//!
//! The plan is built from the manager's records, never from the frontend: the
//! frontend only picks item ids out of a preview, and running the cleanup plans
//! again and removes only ids that are still removable at that moment.
//!
//! What is kept:
//! - a package archive any profile still uses, archived profiles included;
//! - everything an unfinished or recovery-required operation might need;
//! - anything that is a link rather than a real file or folder.
//!
//! Removing a package archive leaves its catalog entry in place, so the record
//! of what was installed survives; installing the same file again restores it.

use crate::api::dto::{CleanupItemDto, CleanupOutcomeDto, CleanupPreviewDto, CleanupResultDto};
use crate::error::{AppError, AppResult};
use crate::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, OperationRepository, ProfileRepository,
};
use crate::ports::storage::{StorageArea, StorageEntry, StorageInventoryPort};
use crate::services::resources::{ResourceClaim, ResourceCoordinator};
use manager_core::ids::OperationId;
use manager_core::operation::ResourceKind;
use manager_core::ports::InstanceLock;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

pub struct StorageCleanupService {
    resources: Arc<ResourceCoordinator>,
    game_repo: Arc<dyn GameInstallationRepository>,
    profile_repo: Arc<dyn ProfileRepository>,
    deployment_repo: Arc<dyn DeploymentRepository>,
    operation_repo: Arc<dyn OperationRepository>,
    storage: Arc<dyn StorageInventoryPort>,
    instance_lock: Arc<dyn InstanceLock>,
}

struct Planned {
    entry: StorageEntry,
    item: CleanupItemDto,
}

fn item_id(entry: &StorageEntry) -> String {
    let area = match entry.area {
        StorageArea::Package => "package",
        StorageArea::SmapiCache => "smapi-cache",
        StorageArea::Staging => "staging",
        StorageArea::Recovery => "recovery",
    };
    match &entry.profile_id {
        Some(profile) => format!("{area}:{profile}:{}", entry.key),
        None => format!("{area}:{}", entry.key),
    }
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

impl StorageCleanupService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resources: Arc<ResourceCoordinator>,
        game_repo: Arc<dyn GameInstallationRepository>,
        profile_repo: Arc<dyn ProfileRepository>,
        deployment_repo: Arc<dyn DeploymentRepository>,
        operation_repo: Arc<dyn OperationRepository>,
        storage: Arc<dyn StorageInventoryPort>,
        instance_lock: Arc<dyn InstanceLock>,
    ) -> Self {
        Self {
            resources,
            game_repo,
            profile_repo,
            deployment_repo,
            operation_repo,
            storage,
            instance_lock,
        }
    }

    /// Package hash -> names of the profiles that still use it.
    fn package_users(&self) -> AppResult<HashMap<String, BTreeSet<String>>> {
        let mut users: HashMap<String, BTreeSet<String>> = HashMap::new();
        for game in self.game_repo.list_games()? {
            for profile in self.profile_repo.list_profiles(&game.id)? {
                for deployment in self
                    .deployment_repo
                    .list_deployments_for_profile(&profile.id)?
                {
                    users
                        .entry(deployment.artifact_hash.as_str().to_ascii_lowercase())
                        .or_default()
                        .insert(profile.name.clone());
                }
            }
        }
        Ok(users)
    }

    /// `lease_held` is true when the caller holds the cleanup's own resource
    /// lease, which must not count as another task running.
    fn plan(&self, lease_held: bool) -> AppResult<(Vec<Planned>, Option<String>)> {
        let entries = self.storage.scan()?;
        let users = self.package_users()?;
        let unresolved = self.operation_repo.list_unresolved_operations()?;
        let unresolved_ids: HashSet<String> =
            unresolved.iter().map(|op| op.id.to_string()).collect();
        let blocked_reason = if !unresolved.is_empty() {
            Some(
                "A change is still pending or needs recovery. Finish or cancel it on the Activity page; until then its files and every unused package are kept."
                    .to_string(),
            )
        } else if !lease_held && !self.resources.is_idle() {
            Some("Another task is running. Try again when it has finished.".to_string())
        } else {
            None
        };
        let busy = blocked_reason.is_some();

        let mut planned = Vec::with_capacity(entries.len());
        for entry in entries {
            let (category, label, detail, removable) = if entry.is_link {
                (
                    "protected",
                    entry.path.display().to_string(),
                    "This is a link, not a file the manager created. Cleanup never follows or removes links.".to_string(),
                    false,
                )
            } else {
                match entry.area {
                    StorageArea::Package => {
                        let label = format!("Package {}", short(&entry.key));
                        match users.get(&entry.key.to_ascii_lowercase()) {
                            Some(profiles) => (
                                "protected",
                                label,
                                format!(
                                    "Used by {}.",
                                    profiles.iter().cloned().collect::<Vec<_>>().join(", ")
                                ),
                                false,
                            ),
                            None if busy => (
                                "protected",
                                label,
                                "Not used by any profile, but kept while another change is pending.".to_string(),
                                false,
                            ),
                            None => (
                                "unused_package",
                                label,
                                "Not used by any profile, including archived ones. Installing the same file again brings it back.".to_string(),
                                true,
                            ),
                        }
                    }
                    StorageArea::SmapiCache => {
                        let label = format!("SMAPI installer files ({})", entry.key);
                        if busy {
                            (
                                "protected",
                                label,
                                "Kept while another task is running.".to_string(),
                                false,
                            )
                        } else {
                            (
                                "cache",
                                label,
                                "Downloaded again when SMAPI is next installed or repaired."
                                    .to_string(),
                                true,
                            )
                        }
                    }
                    StorageArea::Staging | StorageArea::Recovery => {
                        let what = if entry.area == StorageArea::Staging {
                            "Prepared files"
                        } else {
                            "Undo copies"
                        };
                        let label = format!("{what} from change {}", short(&entry.key));
                        let operation = OperationId::from_str(&entry.key)
                            .ok()
                            .map(|id| self.operation_repo.get_operation(&id))
                            .transpose()?
                            .flatten();
                        match operation {
                            _ if unresolved_ids.contains(&entry.key) => (
                                "protected",
                                label,
                                "That change is not finished or needs recovery, and may still need these files.".to_string(),
                                false,
                            ),
                            Some(op) if !op.state.is_terminal() => (
                                "protected",
                                label,
                                "That change is not finished, and may still need these files."
                                    .to_string(),
                                false,
                            ),
                            _ if busy => (
                                "protected",
                                label,
                                "Kept while another change is pending.".to_string(),
                                false,
                            ),
                            Some(_) => (
                                "operation_leftover",
                                label,
                                "That change has finished, so nothing will use these files again."
                                    .to_string(),
                                true,
                            ),
                            None => (
                                "operation_leftover",
                                label,
                                "No change with this id is recorded, so nothing can use these files."
                                    .to_string(),
                                true,
                            ),
                        }
                    }
                }
            };
            let item = CleanupItemDto {
                id: item_id(&entry),
                category: category.to_string(),
                label,
                detail,
                size_bytes: entry.size_bytes,
                removable,
            };
            planned.push(Planned { entry, item });
        }
        planned.sort_by(|a, b| {
            b.item
                .removable
                .cmp(&a.item.removable)
                .then(b.item.size_bytes.cmp(&a.item.size_bytes))
                .then(a.item.id.cmp(&b.item.id))
        });
        Ok((planned, blocked_reason))
    }

    pub fn preview(&self) -> AppResult<CleanupPreviewDto> {
        let (planned, blocked_reason) = self.plan(false)?;
        let reclaimable_bytes = planned
            .iter()
            .filter(|p| p.item.removable)
            .map(|p| p.item.size_bytes)
            .sum();
        let protected_bytes = planned
            .iter()
            .filter(|p| !p.item.removable)
            .map(|p| p.item.size_bytes)
            .sum();
        Ok(CleanupPreviewDto {
            items: planned.into_iter().map(|p| p.item).collect(),
            reclaimable_bytes,
            protected_bytes,
            blocked_reason,
        })
    }

    /// Removes the requested items that are still removable now.
    ///
    /// Every requested id gets an outcome. A failure on one item does not stop
    /// the others, and the result is only complete when all were removed.
    pub fn run(&self, ids: &[String]) -> AppResult<CleanupResultDto> {
        let requested: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        if requested.is_empty() {
            return Err(AppError::validation(
                "CLEANUP_NOTHING_SELECTED",
                "Choose at least one item to remove",
            ));
        }
        // Hold the operation locks for everything that might be touched, so no
        // install, removal or SMAPI change can start while files are deleted.
        let mut claims = Vec::new();
        for game in self.game_repo.list_games()? {
            claims.push(ResourceClaim::write(
                ResourceKind::GameInstallation,
                game.id.to_string(),
            ));
            for profile in self.profile_repo.list_profiles(&game.id)? {
                claims.push(ResourceClaim::write(
                    ResourceKind::Profile,
                    profile.id.to_string(),
                ));
            }
        }
        let _lease = self.resources.try_acquire(&claims)?;
        let _guard = self
            .instance_lock
            .acquire_guard()
            .map_err(AppError::instance_locked)?;

        // Plan again under the locks: the preview may be out of date.
        let (planned, _) = self.plan(true)?;
        let by_id: HashMap<&str, &Planned> =
            planned.iter().map(|p| (p.item.id.as_str(), p)).collect();

        let mut outcomes = Vec::with_capacity(requested.len());
        let mut reclaimed_bytes = 0;
        for id in requested {
            let Some(planned) = by_id.get(id) else {
                outcomes.push(CleanupOutcomeDto {
                    id: id.to_string(),
                    label: id.to_string(),
                    outcome: "skipped".into(),
                    message: Some("It is no longer there.".into()),
                    size_bytes: 0,
                });
                continue;
            };
            let item = &planned.item;
            if !item.removable {
                outcomes.push(CleanupOutcomeDto {
                    id: item.id.clone(),
                    label: item.label.clone(),
                    outcome: "skipped".into(),
                    message: Some(format!("No longer safe to remove. {}", item.detail)),
                    size_bytes: item.size_bytes,
                });
                continue;
            }
            match self.storage.remove(&planned.entry) {
                Ok(()) => {
                    reclaimed_bytes += item.size_bytes;
                    outcomes.push(CleanupOutcomeDto {
                        id: item.id.clone(),
                        label: item.label.clone(),
                        outcome: "removed".into(),
                        message: None,
                        size_bytes: item.size_bytes,
                    });
                }
                Err(error) => outcomes.push(CleanupOutcomeDto {
                    id: item.id.clone(),
                    label: item.label.clone(),
                    outcome: "failed".into(),
                    message: Some(match &error.technical_details {
                        Some(details) => format!("{} ({details})", error.summary),
                        None => error.summary.clone(),
                    }),
                    size_bytes: item.size_bytes,
                }),
            }
        }
        let complete = outcomes.iter().all(|o| o.outcome == "removed");
        Ok(CleanupResultDto {
            outcomes,
            reclaimed_bytes,
            complete,
        })
    }
}
