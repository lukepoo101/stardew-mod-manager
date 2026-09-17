//! In-process resource coordination for live mutations.
//!
//! Two different questions are answered by two different mechanisms:
//!
//! * the in-memory coordinator here answers "what is executing in this process
//!   right now?", giving fine-grained read/write exclusion between concurrent
//!   operations on disjoint profiles or game installations;
//! * the persisted `operation_resources` gate answers "what unresolved durable
//!   work owns this resource?", which survives a restart;
//! * the cross-process file lock still answers "is another application instance
//!   using these files?".
//!
//! Neither replaces the other. This is deliberately in-process only: there is no
//! distributed locking here, and no queue. A conflicting request fails fast.

use crate::error::{AppError, AppResult};
use crate::ports::repositories::OperationRepository;
use manager_core::ids::{OperationId, ProfileId};
use manager_core::operation::{AccessMode, OperationResource, ResourceKind};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

/// A resource one unit of work wants to hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceClaim {
    pub kind: ResourceKind,
    pub resource_id: String,
    pub mode: AccessMode,
}

impl ResourceClaim {
    pub fn read(kind: ResourceKind, resource_id: impl Into<String>) -> Self {
        Self {
            kind,
            resource_id: resource_id.into(),
            mode: AccessMode::Read,
        }
    }

    pub fn write(kind: ResourceKind, resource_id: impl Into<String>) -> Self {
        Self {
            kind,
            resource_id: resource_id.into(),
            mode: AccessMode::Write,
        }
    }
}

/// Anything that occupies a resource, so the conflict rule is written once.
pub trait ResourceHolder {
    fn holder_kind(&self) -> ResourceKind;
    fn holder_id(&self) -> &str;
    fn holder_mode(&self) -> AccessMode;
}

impl ResourceHolder for OperationResource {
    fn holder_kind(&self) -> ResourceKind {
        self.resource_kind
    }
    fn holder_id(&self) -> &str {
        &self.resource_id
    }
    fn holder_mode(&self) -> AccessMode {
        self.access_mode
    }
}

impl ResourceHolder for ResourceClaim {
    fn holder_kind(&self) -> ResourceKind {
        self.kind
    }
    fn holder_id(&self) -> &str {
        &self.resource_id
    }
    fn holder_mode(&self) -> AccessMode {
        self.mode
    }
}

/// The single read/write conflict rule.
///
/// Read + Read is allowed; anything involving a Write conflicts on the same
/// resource identity. Different resource identities are independent.
pub fn claims_conflict(requested: &ResourceClaim, held: &impl ResourceHolder) -> bool {
    requested.kind == held.holder_kind()
        && requested.resource_id == held.holder_id()
        && (requested.mode == AccessMode::Write || held.holder_mode() == AccessMode::Write)
}

/// The first holder that conflicts with any of the requested claims.
pub fn conflicting_holder<'a, H: ResourceHolder>(
    requested: &[ResourceClaim],
    held: impl IntoIterator<Item = &'a H>,
) -> Option<&'a H> {
    held.into_iter().find(|holder| {
        requested
            .iter()
            .any(|claim| claims_conflict(claim, *holder))
    })
}

#[derive(Debug)]
struct HeldClaim {
    kind: ResourceKind,
    resource_id: String,
    mode: AccessMode,
}

impl ResourceHolder for HeldClaim {
    fn holder_kind(&self) -> ResourceKind {
        self.kind
    }
    fn holder_id(&self) -> &str {
        &self.resource_id
    }
    fn holder_mode(&self) -> AccessMode {
        self.mode
    }
}

/// The durable gate for a profile write.
///
/// Shared by every service that starts a profile mutation, so the conflict rule
/// and the reported error cannot drift between them.
pub fn ensure_profile_write_available(
    operation_repo: &dyn OperationRepository,
    profile_id: &ProfileId,
    current_operation: Option<&OperationId>,
) -> AppResult<()> {
    let requested = [ResourceClaim::write(
        ResourceKind::Profile,
        profile_id.to_string(),
    )];
    let held = operation_repo
        .list_unresolved_resources(ResourceKind::Profile, &profile_id.to_string())?
        .into_iter()
        .filter(|resource| Some(&resource.operation_id) != current_operation)
        .collect::<Vec<_>>();

    match conflicting_holder(&requested, held.iter()) {
        None => Ok(()),
        Some(conflict) => Err(AppError::profile_operation_unresolved(
            profile_id,
            &conflict.operation_id,
        )),
    }
}

/// The durable gate for an arbitrary claim set.
pub fn ensure_resources_available(
    operation_repo: &dyn OperationRepository,
    requested: &[ResourceClaim],
    current_operation: Option<&OperationId>,
) -> AppResult<()> {
    let mut held = Vec::new();
    for claim in requested {
        held.extend(
            operation_repo
                .list_unresolved_resources(claim.kind, &claim.resource_id)?
                .into_iter()
                .filter(|resource| Some(&resource.operation_id) != current_operation),
        );
    }
    let Some(conflict) = conflicting_holder(requested, held.iter()) else {
        return Ok(());
    };
    if conflict.resource_kind == ResourceKind::Profile {
        if let Ok(profile_id) = ProfileId::from_str(&conflict.resource_id) {
            return Err(AppError::profile_operation_unresolved(
                &profile_id,
                &conflict.operation_id,
            ));
        }
    }
    Err(AppError::resource_blocked_by_unresolved_operation(
        conflict.resource_kind,
        &conflict.resource_id,
        &conflict.operation_id,
    ))
}

/// The held state of one resource key.
#[derive(Debug, Default)]
struct Held {
    readers: usize,
    writers: usize,
}

fn resource_key(kind: ResourceKind, resource_id: &str) -> String {
    format!("{:?}:{}", kind, resource_id)
}

/// The in-process resource coordinator.
#[derive(Debug)]
pub struct ResourceCoordinator {
    held: Mutex<HashMap<String, Held>>,
}

impl Default for ResourceCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceCoordinator {
    pub fn new() -> Self {
        Self {
            held: Mutex::new(HashMap::new()),
        }
    }

    /// Acquires every requested claim atomically.
    ///
    /// The whole set is taken under one mutex, so two operations can never
    /// deadlock against each other by acquiring disjoint resources in opposite
    /// orders, and a failed acquisition leaves no partial claims behind.
    pub fn try_acquire(self: &Arc<Self>, claims: &[ResourceClaim]) -> AppResult<ResourceLease> {
        let mut held = self
            .held
            .lock()
            .map_err(|e| AppError::internal("Resource coordinator is poisoned", e.to_string()))?;

        let existing: Vec<HeldClaim> = held
            .iter()
            .flat_map(|(key, state)| {
                let mut claims = Vec::new();
                for _ in 0..state.readers {
                    claims.push(HeldClaim {
                        kind: parse_kind(key),
                        resource_id: parse_id(key),
                        mode: AccessMode::Read,
                    });
                }
                for _ in 0..state.writers {
                    claims.push(HeldClaim {
                        kind: parse_kind(key),
                        resource_id: parse_id(key),
                        mode: AccessMode::Write,
                    });
                }
                claims
            })
            .collect();

        if let Some(conflict) = conflicting_holder(claims, existing.iter()) {
            return Err(AppError::resource_busy(
                conflict.kind,
                &conflict.resource_id,
            ));
        }

        for claim in claims {
            let entry = held
                .entry(resource_key(claim.kind, &claim.resource_id))
                .or_default();
            match claim.mode {
                AccessMode::Read => entry.readers += 1,
                AccessMode::Write => entry.writers += 1,
            }
        }

        Ok(ResourceLease {
            coordinator: Arc::clone(self),
            claims: claims.to_vec(),
        })
    }

    fn release(&self, claims: &[ResourceClaim]) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        for claim in claims {
            let key = resource_key(claim.kind, &claim.resource_id);
            if let Some(entry) = held.get_mut(&key) {
                match claim.mode {
                    AccessMode::Read => entry.readers = entry.readers.saturating_sub(1),
                    AccessMode::Write => entry.writers = entry.writers.saturating_sub(1),
                }
                if entry.readers == 0 && entry.writers == 0 {
                    held.remove(&key);
                }
            }
        }
    }

    /// Whether nothing is currently held, for tests and diagnostics.
    pub fn is_idle(&self) -> bool {
        match self.held.lock() {
            Ok(held) => held.is_empty(),
            // A poisoned coordinator is not idle: treating it as idle would let
            // a conflicting claim through.
            Err(_) => false,
        }
    }
}

/// Releases its claims when dropped.
#[derive(Debug)]
pub struct ResourceLease {
    coordinator: Arc<ResourceCoordinator>,
    claims: Vec<ResourceClaim>,
}

impl Drop for ResourceLease {
    fn drop(&mut self) {
        self.coordinator.release(&self.claims);
    }
}

fn parse_kind(key: &str) -> ResourceKind {
    match key.split(':').next() {
        Some("Profile") => ResourceKind::Profile,
        Some("GameInstallation") => ResourceKind::GameInstallation,
        _ => ResourceKind::Artifact,
    }
}

fn parse_id(key: &str) -> String {
    key.split_once(':')
        .map(|(_, id)| id.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coordinator() -> Arc<ResourceCoordinator> {
        Arc::new(ResourceCoordinator::new())
    }

    #[test]
    fn two_readers_of_the_same_resource_coexist() {
        let coordinator = coordinator();
        let first = coordinator
            .try_acquire(&[ResourceClaim::read(ResourceKind::Profile, "a")])
            .unwrap();
        let second = coordinator
            .try_acquire(&[ResourceClaim::read(ResourceKind::Profile, "a")])
            .unwrap();
        assert!(!coordinator.is_idle());
        drop(first);
        assert!(!coordinator.is_idle());
        drop(second);
        assert!(coordinator.is_idle());
    }

    #[test]
    fn a_write_conflicts_with_readers_and_writers() {
        let coordinator = coordinator();
        let _read = coordinator
            .try_acquire(&[ResourceClaim::read(ResourceKind::Profile, "a")])
            .unwrap();
        let read_vs_write = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "a")])
            .unwrap_err();
        assert_eq!(read_vs_write.code, "RESOURCE_BUSY");
        assert_eq!(
            read_vs_write.category,
            crate::error::AppErrorCategory::OperationConflict
        );
        assert_eq!(
            read_vs_write.recoverability,
            crate::error::Recoverability::Retryable
        );
        drop(_read);

        let _write = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "a")])
            .unwrap();
        let write_vs_read = coordinator
            .try_acquire(&[ResourceClaim::read(ResourceKind::Profile, "a")])
            .unwrap_err();
        assert_eq!(write_vs_read.code, "RESOURCE_BUSY");
        let write_vs_write = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "a")])
            .unwrap_err();
        assert_eq!(write_vs_write.code, "RESOURCE_BUSY");
    }

    #[test]
    fn disjoint_profiles_and_game_installations_do_not_conflict() {
        let coordinator = coordinator();
        let _a = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "a")])
            .unwrap();
        let _b = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "b")])
            .unwrap();
        let _game = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::GameInstallation, "g")])
            .unwrap();
    }

    #[test]
    fn multi_resource_acquisition_is_atomic() {
        let coordinator = coordinator();
        let _held = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "b")])
            .unwrap();

        let failed = coordinator.try_acquire(&[
            ResourceClaim::write(ResourceKind::Profile, "a"),
            ResourceClaim::write(ResourceKind::Profile, "b"),
        ]);
        assert!(failed.is_err(), "one conflicting claim fails the whole set");

        // No partial claim was left behind for the resource that was free.
        let lease = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "a")])
            .unwrap();
        drop(lease);
    }

    #[test]
    fn dropping_a_lease_releases_every_claim() {
        let coordinator = coordinator();
        {
            let _lease = coordinator
                .try_acquire(&[
                    ResourceClaim::write(ResourceKind::Profile, "a"),
                    ResourceClaim::read(ResourceKind::GameInstallation, "g"),
                ])
                .unwrap();
        }
        assert!(coordinator.is_idle());
        let _again = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "a")])
            .unwrap();
    }

    #[test]
    fn a_recovery_write_conflicts_with_a_launch_read_of_the_same_profile() {
        let coordinator = coordinator();
        let recovery = coordinator
            .try_acquire(&[ResourceClaim::write(ResourceKind::Profile, "a")])
            .unwrap();
        let launch = coordinator
            .try_acquire(&[ResourceClaim::read(ResourceKind::Profile, "a")])
            .unwrap_err();
        assert_eq!(launch.code, "RESOURCE_BUSY");
        drop(recovery);

        // A launch on a different profile is unaffected.
        let _other = coordinator
            .try_acquire(&[ResourceClaim::read(ResourceKind::Profile, "b")])
            .unwrap();
    }

    #[test]
    fn the_durable_conflict_rule_matches_the_coordinator_rule() {
        let durable = manager_core::operation::OperationResource {
            operation_id: manager_core::ids::OperationId::new(),
            resource_kind: ResourceKind::Profile,
            resource_id: "a".to_string(),
            access_mode: AccessMode::Write,
        };
        assert!(claims_conflict(
            &ResourceClaim::read(ResourceKind::Profile, "a"),
            &durable
        ));
        assert!(!claims_conflict(
            &ResourceClaim::read(ResourceKind::Profile, "b"),
            &durable
        ));
        assert!(!claims_conflict(
            &ResourceClaim::read(ResourceKind::GameInstallation, "a"),
            &durable
        ));

        let read_only = manager_core::operation::OperationResource {
            access_mode: AccessMode::Read,
            ..durable
        };
        assert!(!claims_conflict(
            &ResourceClaim::read(ResourceKind::Profile, "a"),
            &read_only
        ));
        assert!(claims_conflict(
            &ResourceClaim::write(ResourceKind::Profile, "a"),
            &read_only
        ));
    }
}
