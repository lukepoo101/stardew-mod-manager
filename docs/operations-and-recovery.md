# Stardew Mod Manager - Operations & Recovery

This is the operational contract of the durable operation engine: what is
persisted, what each state means, how restart recovery decides, and what an error
promises. It describes current behaviour, not a migration plan.

## Plan schema versions

`operations.plan_schema_version` is the compatibility switch, not a database
schema version.

| Version | Written by | Recovery |
| --- | --- | --- |
| `0`/historical | the pre-architecture runtime, reconciled by migration 0007 | migrated historical-operation reconciliation |
| `1` (`OPERATION_PLAN_SCHEMA_V1`) | builds before the durable step engine | conservative plan-v1 compatibility reconciliation |
| `2` (`OPERATION_PLAN_SCHEMA_V2`) | this build | deterministic step-based engine |

Only new operations use v2. Existing v1 rows are never rewritten, and an
unsupported plan schema fails safely without mutating anything.

## Persisted steps

`operation_steps` records the boundaries an operation crossed. For v2:

```text
install:  1 retain_artifact          2 inspect_and_stage      3 verify_staged
          4 publish_deployment        5 commit_install_database 6 cleanup_staging
          7 quarantine_published_deployment   (compensation, only when needed)

removal:  1 quarantine_deployment     2 commit_removal_database
          3 restore_quarantined_deployment    (compensation, only when needed)

SMAPI:    1 download_smapi_installer  2 install_smapi_files    3 persist_smapi_state
```

Each live side effect is bracketed: the step is persisted `Running` before the
effect is attempted, and `Completed` only once evidence confirms it happened. A
compensation step is persisted only when compensation actually becomes necessary
- there are no pre-created rollback steps pretending to have succeeded. Step
payloads carry only trusted, ID-derived evidence (artifact hash, profile id,
relative deployment folder, expected revision, observed version); never an
arbitrary absolute managed path.

## State lifecycle

```text
Draft -> Prepared -> Running -> Committing -> Succeeded
                 \          \            \
                  \          -> Failed    -> RecoveryRequired
                   -> Cancelled
```

- `Draft`: inspection and staging in progress; nothing live has happened. Safe to
  cancel and clean up.
- `Prepared`: the semantic plan is frozen. `plan_json`,
  `plan_schema_version`, the operation kind and `expected_profile_revision` are
  never rewritten from here on.
- `Running` / `Committing`: live filesystem moves and database transitions are
  underway.
- `Succeeded`: written only when both filesystem and database state are
  consistent.
- `Failed`: the operation ended without a live side effect that needs attention.
- `RecoveryRequired`: automated recovery could not prove a safe terminal state.

Every runtime transition is validated against the state machine and written
through one lifecycle helper. The single exception is the atomic
`Committing -> Succeeded` inside an install/removal commit, which happens in the
same SQLite transaction as the semantic mutation and asserts its expected
previous state. No authoritative transition write is discarded: if persisting
`RecoveryRequired` itself fails, the returned error says the state is
untrustworthy and carries the operation id and the original failure.

## Resource locking

Three mechanisms, three questions:

- `operation_resources` (persisted) - which unresolved durable work owns this
  resource. This survives a restart, so a historical `RecoveryRequired`
  operation still blocks a conflicting new write after the application restarts.
- The in-process resource coordinator - what is executing in this process now.
  Read + Read coexist; anything involving a Write conflicts on the same resource
  identity; disjoint profiles or game installations proceed independently. A
  conflicting request fails fast with `RESOURCE_BUSY` (retryable). There is no
  queue.
- `FileInstanceLock` - cross-process exclusion. It is re-entrant within one
  process, so two disjoint local operations do not look like a second instance to
  each other, while a genuinely different process is still excluded.

Operations acquire their persisted claims before entering live execution or
recovery and hold them for the duration. A launch takes transient `Read` claims
on its profile and game installation; SMAPI setup takes a `Write` claim on the
game installation. Launch and SMAPI participate without inventing operation rows.

## Restart recovery

At startup, unresolved operations are enumerated and reconciled individually. A
single ambiguous operation never stops the application from opening: its evidence
is persisted on the operation and the bootstrap read model keeps reporting it.
Only a failure that prevents recovery processing itself - such as being unable to
read the operations table - is fatal. If another process holds the instance lock,
or the game is running, reconciliation is deferred rather than forced.

Recovery is routed by provenance: migrated historical operations, plan-v1
operations, and plan-v2 operations each use their own reconciler.

For v2 the decision uses the operation state, its persisted steps, live
deployment evidence, recovery-tree evidence, authoritative database ownership and
the profile revision:

- publication that never completed is retried when its staged source is still
  valid, and otherwise ends as a terminal failure that asks for a fresh plan;
- a live folder the database does not own is either adopted by the atomic commit
  (when the prepared revision is still current) or compensated by being moved
  into the recovery tree;
- a quarantined deployment whose database commit never landed is committed;
- a database commit that already happened completes the operation;
- ambiguous evidence - both copies present, or a deployment missing while the
  database still owns it - requires manual reconciliation.

### Evidence semantics

`Ok(true)` is present, `Ok(false)` is absent, and `Err` means the system cannot
tell. An unreadable evidence query leaves the operation in `RecoveryRequired`
instead of being treated as absence, because `Failed` would claim more than the
system knows.

## RecoveryRequired semantics

While an operation is `RecoveryRequired`:

- its filesystem evidence - staging, quarantine and recovery folders - is
  preserved;
- the operations that declared a conflicting resource are blocked;
- the errors returned for it are `category = recovery` and
  `recoverability = requires_manual_intervention`, with `operation_id` set to the
  affected operation. The original diagnosis (code, summary, technical details)
  is preserved: only the recovery semantics are promoted.

Recovery is never cleared merely to make the application usable.

## Error and recovery contract

Every product command returns `IpcResult<T>`; a rejected invoke reaches the
frontend as `ApiClientError` carrying code, category, recoverability, summary,
technical details and operation id. Stable codes relevant here include:

| Code | Meaning |
| --- | --- |
| `RESOURCE_BUSY` | another in-process operation holds the resource; retryable |
| `PROFILE_OPERATION_UNRESOLVED` | an unresolved durable operation owns the profile; manual intervention |
| `PREVIEW_STALE` | the profile moved past the prepared revision; retry with a fresh plan |
| `PERSISTED_*_INVALID` | a persisted value could not be decoded; storage error, no mutation |
| `RECOVERY_STATE_PERSIST_FAILED` | the operation needs manual reconciliation and that fact could not be recorded |
| `UNSUPPORTED_OPERATION_PLAN_SCHEMA` | recovery does not know this plan schema; nothing was mutated |

## Historical operation compatibility

Published migrations (including migration 0007), the legacy database upgrade
tests, migrated historical-operation reconciliation and plan-schema-v1
reconciliation remain in place. They exist so databases written by older released
versions still open and are reconciled safely. They never create new operations,
and they are not an alternate runtime architecture.

## Stale-plan protection

Each profile has a monotonically increasing revision. A prepared operation
captures the expected revision, and the authoritative commit compares it inside
the SQLite transaction. If the profile changed after the preview, the commit
fails with an operation conflict and the user must prepare a fresh plan.

## Artifact retention

Newly selected archives are copied into content-addressed manager storage before
install preparation. Installations whose original archive was never retained
migrate with metadata-only placeholder artifact rows, so the installed deployment
stays representable without pretending its source bytes exist.
