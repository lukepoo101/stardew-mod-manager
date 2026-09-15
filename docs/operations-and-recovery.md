# Stardew Mod Manager — Operations & Recovery

PR #317 introduces the durable operation data model and the first application-layer install/remove workflows. The full step-by-step resume engine is still transitional; ADR-0013 defines the accepted target and the remaining completion work.

## Current guarantees

- Install/remove previews have durable operation identity instead of relying only on transient UI state.
- Operations record affected game/profile resources.
- Profile mutations carry an `expected_profile_revision`; stale commits fail rather than silently applying an outdated plan.
- New install sources are retained before archive inspection/staging.
- Install/remove database records and semantic `OperationEffect` rows are committed atomically.
- Filesystem/DB split-brain during install/remove is compensated when the application can safely restore consistency.
- If an interrupted operation cannot be safely proven complete or compensated, recovery evidence is preserved and the operation becomes `RecoveryRequired`.

## Operation lifecycle target

```text
Draft -> Prepared -> Running -> Committing -> Succeeded
                 \          \            \
                  \          -> Failed    -> RecoveryRequired
                   -> Cancelled
```

The completed engine centrally validates those transitions and persists meaningful side-effect steps before and after execution. PR #317 persists retention/inspection steps plus the operation/resource/effect model, but it does not yet claim complete persisted-step coverage for every execution boundary.

## Persisted resources and effects

`operation_resources` records the resource kind, identity and access mode for an operation. This is the durable input for resource-scoped conflict/recovery behavior.

`operation_effects` records semantic changes in the same SQLite transaction as the authoritative state mutation, providing Activity history and future diagnostic correlation.

## Recovery policy

Recovery prefers certainty over convenience:

1. Never report success solely because a process or filesystem action was attempted.
2. Reconcile both database and filesystem evidence.
3. Compensate only where the operation type has a safe inverse and the required source state can be proven.
4. Preserve staging/recovery/quarantine evidence when compensation is uncertain.
5. Scope recovery impact to the affected game/profile resource wherever the current implementation can safely do so.

## Stale-plan protection

Each profile has a monotonically increasing revision. A prepared operation captures the expected revision; authoritative commits compare it inside the SQLite transaction. If the profile changed after preview, the commit fails with an operation conflict and the user must prepare a fresh plan.

## Artifact retention

Newly selected archives are copied into content-addressed manager storage before install preparation. Legacy installations whose original archive was never retained migrate with metadata-only placeholder artifact rows so the installed deployment remains representable without pretending source bytes exist.

## Follow-up completion work

The operation migration is complete when:
- every install/remove execution side effect has persisted `OperationStep` state;
- legal state transitions are centrally enforced;
- an in-process resource coordinator blocks conflicting live mutations while permitting disjoint work;
- startup recovery deterministically resumes, completes or compensates supported operation kinds from persisted steps;
- the legacy MVP recovery path and global pending-operation guard are removed.
