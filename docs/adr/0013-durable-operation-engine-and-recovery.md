# ADR-0013: Durable Operation Engine, Step Persistence, and Resource-Scoped Recovery

## Status
Accepted — implemented

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
The engine transitions strictly through validated states:
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
Fine-grained execution steps are persisted per operation kind with individual
statuses, timestamps and error payloads. The install lifecycle is
`retain_artifact`, `inspect_and_stage`, `verify_staged`,
`publish_deployment`, `commit_install_database`, `cleanup_staging`; removal is
`quarantine_deployment`, `commit_removal_database`; SMAPI setup is
`download_smapi_installer`, `install_smapi_files`, `persist_smapi_state`.
Compensation steps (`quarantine_published_deployment`,
`restore_quarantined_deployment`) are persisted only when compensation actually
becomes necessary.

Recovery evaluates completed steps together with concrete filesystem and
database evidence rather than inferring progress from the coarse operation
state.

### 3. Persisted Resources (`operation_resources`)
Each operation records affected resources in `operation_resources` (`operation_id`, `resource_kind`, `resource_id`, `access_mode` [Read, Write]).
An in-process resource lock coordinator prevents concurrent conflicting
mutations on the same profile or game installation while allowing concurrent
access to disjoint resources (for example, Profile A can launch while Profile B
recovers). It holds read/write claims for the duration of a live mutation, fails
fast with `RESOURCE_BUSY` rather than queueing, and is deliberately in-process
only. The persisted `operation_resources` rows remain the restart-surviving half
of the same conflict rule, and the cross-process `FileInstanceLock` remains the
other-process guard.

### 4. Stale-Plan Protection via Profile Revision
Prepared operations targeting a profile record `expected_profile_revision`. If another operation commits and increments `profiles.revision` before this operation commits, the commit is rejected with `OperationConflict`, prompting the user to review a refreshed preview.

### 5. Semantic Effects (`operation_effects`)
Authoritative database commits insert typed `OperationEffect` rows in the same SQLite transaction (e.g., `ProfileComponentAdded`, `ProfileComponentRemoved`, `ProfileCreated`, `SmapiRuntimeChanged`). This powers the Activity feed and provides the foundation for "what changed?" diagnostics.

### 6. Trusted Path Resolution
Operations never record user-supplied or arbitrary absolute managed paths. All target roots, staging folders, and recovery folders are derived deterministically through `AppPaths` from trusted IDs.

## Implementation status

This ADR is implemented. The runtime guarantees are:

- every live install, removal and SMAPI side effect has persisted step
  boundaries: `Running` before the effect is attempted, `Completed` only once
  evidence confirms it, and compensation steps persisted only when compensation
  becomes necessary;
- v2 recovery is deterministic from persisted steps plus concrete filesystem and
  database evidence, and an unreadable evidence query is never treated as
  absence;
- operation state transitions are validated centrally, and no authoritative
  transition write is silently discarded;
- an in-process read/write resource coordinator enforces same-process conflicts
  while permitting disjoint work, and the persisted `operation_resources` claims
  keep blocking conflicting writes across a restart;
- cross-process instance exclusion remains, and is now re-entrant within one
  process;
- plan-schema-v1 and migrated historical operations use isolated compatibility
  reconciliation and are never rewritten into the v2 engine;
- unknown persisted enum values are not silently coerced: they fail as explicit
  storage errors.

The legacy MVP recovery engine and its compatibility client were removed in
earlier work; the executable path runs entirely on the modern service graph.
Historical migrated-data reconciliation intentionally remains: it exists so
databases written by older released versions still open and are reconciled
safely, and it never creates new operations.

## Consequences
### Positive
- Durable operation identity, resource scope, stale-plan protection and semantic history.
- Recovery prefers proven evidence over convenience, and preserves that evidence instead of guessing after interrupted live mutations.
- Recovery is deterministic from persisted steps for v2 operations.
- Multi-profile isolation is explicit, so recovery issues are scoped rather than inherently global.

### Negative
- Additional database writes per operation step.
- Careful transaction and error-state management is required to keep step state, effects and filesystem mutations synchronized.
- Some interrupted operations still stop at `RecoveryRequired` on purpose: when the evidence is genuinely ambiguous, asking a human is safer than pretending to know.
