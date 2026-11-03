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
use manager_core::operation::{
    AccessMode, Operation, OperationKind, OperationResource, ResourceKind,
};
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

/// Another window, process or command is carrying out a change that holds
/// the resource. Waiting for it is enough.
pub const CHANGE_IN_PROGRESS: &str = "CHANGE_IN_PROGRESS";

/// Durable claims that block another change to a resource.
///
/// These are the claims of unresolved operations, except previews. A preview
/// (an inspected, staged install or removal nobody has confirmed yet) changed
/// nothing and is not being carried out, so it never holds a profile against
/// a toggle, another preview or a change from the command line: the profile
/// revision it was prepared against already stops it from being confirmed over
/// a change it did not see, and the change that moves the revision cancels it.
pub fn blocking_unresolved_resources(
    operation_repo: &dyn OperationRepository,
    kind: ResourceKind,
    resource_id: &str,
) -> AppResult<Vec<OperationResource>> {
    let mut blocking = Vec::new();
    for resource in operation_repo.list_unresolved_resources(kind, resource_id)? {
        let preview = operation_repo
            .get_operation(&resource.operation_id)?
            .is_some_and(|op| op.state.is_preview());
        if !preview {
            blocking.push(resource);
        }
    }
    Ok(blocking)
}

/// Whether the process that owns an unfinished operation is still carrying
/// it out.
///
/// "Unfinished" alone says nothing about that: the app, a second command or
/// a killed command can all leave the same Committing row. What proves an
/// owner is alive is what it holds:
///
/// * this process holds a lease on one of the operation's claims, or began a
///   change set and has not finished it;
/// * another process holds the cross-process instance lock, which every
///   change holds while it moves files (the CLI included);
/// * a change set, whose parts an app window drives one command at a time,
///   is alive while a window is open (the command line's view).
///
/// Anything else was interrupted: its owner stopped, and recovery (which
/// takes the instance lock itself) is the way forward.
pub struct OperationLiveness {
    resources: Arc<ResourceCoordinator>,
    instance_lock: Arc<dyn manager_core::ports::InstanceLock>,
    window_elsewhere: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl OperationLiveness {
    pub fn new(
        resources: Arc<ResourceCoordinator>,
        instance_lock: Arc<dyn manager_core::ports::InstanceLock>,
    ) -> Self {
        Self {
            resources,
            instance_lock,
            window_elsewhere: None,
        }
    }

    /// For a process that is not the app window (the command line): how to
    /// tell whether a window is open.
    pub fn with_window_probe(mut self, probe: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        self.window_elsewhere = Some(probe);
        self
    }

    /// Whether `op`'s owner is still carrying it out. Previews and
    /// operations waiting for recovery have no owner carrying them out.
    pub fn owner_alive(&self, op: &Operation, claims: &[ResourceClaim]) -> bool {
        if op.state.is_preview() || op.state.requires_recovery() || op.state.is_terminal() {
            return false;
        }
        if self.resources.is_carrying_out(&op.id, claims) {
            return true;
        }
        if op.kind == OperationKind::ProfileChangeSet {
            return self.window_elsewhere.as_ref().is_some_and(|probe| probe());
        }
        self.instance_lock.held_by_another_process()
    }

    /// `owner_alive`, reading the operation's claims from the journal.
    pub fn owner_alive_for(
        &self,
        operation_repo: &dyn OperationRepository,
        op: &Operation,
    ) -> bool {
        let claims: Vec<ResourceClaim> = operation_repo
            .list_operation_resources(&op.id)
            .map(|resources| {
                resources
                    .into_iter()
                    .map(|r| ResourceClaim {
                        kind: r.resource_kind,
                        resource_id: r.resource_id,
                        mode: r.access_mode,
                    })
                    .collect()
            })
            // Unreadable claims: fall back to the cross-process evidence.
            .unwrap_or_default();
        self.owner_alive(op, &claims)
    }
}

/// The refusal for a change blocked by another operation's durable claim.
///
/// It names what is in the way. A change whose owner is still carrying it
/// out only needs waiting for; anything else (waiting for recovery, or left
/// behind by a process that stopped) needs recovery, which Activity offers.
/// Without liveness evidence (`liveness` is None) an unfinished operation is
/// assumed to be in progress, the cautious reading.
fn blocked_by(
    operation_repo: &dyn OperationRepository,
    liveness: Option<&OperationLiveness>,
    conflict: &OperationResource,
) -> AppError {
    let operation = operation_repo
        .get_operation(&conflict.operation_id)
        .ok()
        .flatten();
    let phrase = operation
        .as_ref()
        .map(|op| crate::services::operation_labels::kind_phrase(op.kind))
        .unwrap_or("an earlier change");
    let profile = if conflict.resource_kind == ResourceKind::Profile {
        ProfileId::from_str(&conflict.resource_id).ok()
    } else {
        None
    };
    let needs_recovery = match &operation {
        None => true,
        Some(op) if op.state.requires_recovery() => true,
        Some(op) => liveness.is_some_and(|l| !l.owner_alive_for(operation_repo, op)),
    };

    let error = if needs_recovery {
        let mut error = match &profile {
            Some(profile_id) => {
                AppError::profile_operation_unresolved(profile_id, &conflict.operation_id)
            }
            None => AppError::resource_blocked_by_unresolved_operation(
                conflict.resource_kind,
                &conflict.resource_id,
                &conflict.operation_id,
            ),
        };
        error.summary = format!(
            "An interrupted change ({phrase}) needs recovery before this can change; Activity lists it with a way to retry"
        );
        error
    } else {
        let target = if profile.is_some() {
            "this profile"
        } else {
            "this game installation"
        };
        AppError::conflict(
            CHANGE_IN_PROGRESS,
            format!("Another change to {target} is still in progress ({phrase})"),
            format!(
                "{:?} '{}' is held by operation {} ({:?}, started {})",
                conflict.resource_kind,
                conflict.resource_id,
                conflict.operation_id,
                operation.as_ref().map(|op| op.state),
                operation
                    .as_ref()
                    .map(|op| op.created_at.to_rfc3339())
                    .unwrap_or_default()
            ),
            crate::error::Recoverability::Retryable,
        )
    };
    error.with_operation_id(conflict.operation_id)
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
    ensure_profile_write_available_with(operation_repo, None, profile_id, current_operation)
}

/// `ensure_profile_write_available`, telling a change in progress from an
/// interrupted one by `liveness`.
pub fn ensure_profile_write_available_with(
    operation_repo: &dyn OperationRepository,
    liveness: Option<&OperationLiveness>,
    profile_id: &ProfileId,
    current_operation: Option<&OperationId>,
) -> AppResult<()> {
    ensure_resources_available_with(
        operation_repo,
        liveness,
        &[ResourceClaim::write(
            ResourceKind::Profile,
            profile_id.to_string(),
        )],
        current_operation,
    )
}

/// The durable gate for an arbitrary claim set.
pub fn ensure_resources_available(
    operation_repo: &dyn OperationRepository,
    requested: &[ResourceClaim],
    current_operation: Option<&OperationId>,
) -> AppResult<()> {
    ensure_resources_available_with(operation_repo, None, requested, current_operation)
}

/// `ensure_resources_available`, telling a change in progress from an
/// interrupted one by `liveness`.
pub fn ensure_resources_available_with(
    operation_repo: &dyn OperationRepository,
    liveness: Option<&OperationLiveness>,
    requested: &[ResourceClaim],
    current_operation: Option<&OperationId>,
) -> AppResult<()> {
    let mut held = Vec::new();
    for claim in requested {
        held.extend(
            blocking_unresolved_resources(operation_repo, claim.kind, &claim.resource_id)?
                .into_iter()
                .filter(|resource| Some(&resource.operation_id) != current_operation),
        );
    }
    match conflicting_holder(requested, held.iter()) {
        None => Ok(()),
        Some(conflict) => Err(blocked_by(operation_repo, liveness, conflict)),
    }
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
    /// Operations this process is carrying out across several commands
    /// (change sets, whose parts the window drives one command at a time).
    carrying_out: Mutex<std::collections::HashSet<OperationId>>,
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
            carrying_out: Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Records that this process is carrying out `operation` until
    /// `finished_carrying_out` is called, even between commands.
    pub fn carrying_out(&self, operation: OperationId) {
        if let Ok(mut set) = self.carrying_out.lock() {
            set.insert(operation);
        }
    }

    pub fn finished_carrying_out(&self, operation: &OperationId) {
        if let Ok(mut set) = self.carrying_out.lock() {
            set.remove(operation);
        }
    }

    /// Whether this process is carrying out `operation` right now: it holds
    /// a lease on one of its claims, or it began the operation and has not
    /// finished it.
    pub fn is_carrying_out(&self, operation: &OperationId, claims: &[ResourceClaim]) -> bool {
        if self
            .carrying_out
            .lock()
            .map(|set| set.contains(operation))
            // A poisoned registry cannot prove the operation is dead.
            .unwrap_or(true)
        {
            return true;
        }
        match self.held.lock() {
            Ok(held) => claims
                .iter()
                .any(|claim| held.contains_key(&resource_key(claim.kind, &claim.resource_id))),
            Err(_) => true,
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
