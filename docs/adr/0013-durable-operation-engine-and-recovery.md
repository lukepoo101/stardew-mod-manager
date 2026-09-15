# ADR-0013: Durable Operation Engine, Step Persistence, and Resource-Scoped Recovery

## Status
Accepted target — transitional implementation

## Date
2026-09-13

## Context
The MVP introduced operation journaling before destructive filesystem changes, recording an `Operation` row with `plan_json` and `state`. While effective for simple single-mod install and remove recoveries, it exhibited key limitations:
1. Operations had coarse states (`Pending`, `Prepared`, `Running`, `Completed`, `Failed`, `Recovering`). Recovery had to reverse-engineer filesystem state from the overall plan rather than deterministic step progress.
2. Inspected package plans were stored only in an ephemeral in-memory cache (`PendingInspectionStore`) with a 30-minute TTL, losing state across restarts.
3. Any unresolved operation globally blocked the entire application (`ensure_no_pending_operations()`), preventing safe operations on unrelated games or profiles.
4. Removal plans persisted arbitrary absolute recovery folder paths, violating path containment principles.
5. There was no audit trail of semantic changes (`OperationEffects`), making it impossible to query activity history or determine what changed between game sessions.

## Decision
Introduce a durable operation engine with step-level persistence, resource-scoped locking, and semantic audit effects:

### 1. Operation Lifecycle
The completed engine will transition strictly through validated states:
```text
Draft -> Prepared -> Running -> Committing -> Succeeded
                 \          \            \
                  \          -> Failed    -> RecoveryRequired
                   -> Cancelled
```
- **`Draft`**: Inspection and staging in progress; no live mod mutations have occurred. Safe to discard or clean up on startup.
- **`Prepared`**: Plan verified and prerequisites checked. The semantic plan is **immutable** once `Prepared`.
- **`Running` / `Committing`**: Live filesystem moves and database transitions are underway.
- **`Succeeded`**: Written only when both filesystem and database state have been fully reconciled.
- **`RecoveryRequired`**: Entered when automated recovery cannot safely prove a clean terminal state, requiring user review.

### 2. Persisted Steps (`operation_steps`)
Fine-grained execution steps are persisted with individual statuses, timestamps, and error payloads:
1. Retain source artifact in immutable store.
2. Inspect archive structure and discover manifests.
3. Stage extracted files to profile staging tree.
4. Verify staged file integrity and reject symlinks.
5. Prepare recovery / backup data.
6. Publish deployment to active profile mods folder.
7. Atomically commit database records (`InstallCommit` or `RemovalCommit`).
8. Clean up staging.

The completed recovery engine evaluates completed steps rather than inferring progress.

### 3. Persisted Resources (`operation_resources`)
Each operation records affected resources in `operation_resources` (`operation_id`, `resource_kind`, `resource_id`, `access_mode` [Read, Write]).
The completed design adds an in-process resource lock coordinator preventing concurrent mutations on the same profile or game installation while allowing concurrent access to disjoint resources (e.g., Profile A can launch while Profile B recovers).

### 4. Stale-Plan Protection via Profile Revision
Prepared operations targeting a profile record `expected_profile_revision`. If another operation commits and increments `profiles.revision` before this operation commits, the commit is rejected with `OperationConflict`, prompting the user to review a refreshed preview.

### 5. Semantic Effects (`operation_effects`)
Authoritative database commits insert typed `OperationEffect` rows in the same SQLite transaction (e.g., `ProfileComponentAdded`, `ProfileComponentRemoved`, `ProfileCreated`, `SmapiRuntimeChanged`). This powers the Activity feed and provides the foundation for "what changed?" diagnostics.

### 6. Trusted Path Resolution
Operations never record user-supplied or arbitrary absolute managed paths. All target roots, staging folders, and recovery folders are derived deterministically through `AppPaths` from trusted IDs.

## Transitional implementation status

PR #317 persists operations, resources, initial inspection/staging steps, semantic effects and expected profile revisions. Install/remove database commits are transactional, and filesystem/DB split-brain is conservatively compensated where possible; when the application cannot prove a safe terminal state, evidence is preserved and the operation becomes `RecoveryRequired`.

The following parts of this ADR are **not yet complete in PR #317** and must not be treated as runtime guarantees yet:
- every execution-side effect represented by a persisted `OperationStep`;
- central enforcement of every state-machine transition;
- an in-process resource lock coordinator;
- deterministic automatic resume/compensation from every persisted crash boundary;
- removal of the legacy MVP recovery engine and compatibility operation paths.

Those are explicit migration-completion items, not behavior silently implied by the presence of the new tables.

## Consequences
### Positive
- Already provides durable operation identity, resource scope, stale-plan protection and semantic history.
- Preserves recovery evidence instead of guessing after interrupted live mutations.
- Once the remaining transition/step coordinator work lands, recovery can become fully idempotent and resumable from persisted steps.
- Multi-profile isolation is represented explicitly so recovery issues can be scoped rather than inherently global.

### Negative
- Additional database writes per operation step.
- Requires careful transaction and error state management to ensure step state and effects remain synchronized with filesystem mutations.
- During the transitional implementation, some interrupted operations intentionally stop at `RecoveryRequired` rather than claiming automatic recovery that has not yet been implemented.
