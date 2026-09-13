# Stardew Mod Manager — Durable Operations & Recovery

## 1. The Operation Engine

All state mutations in Stardew Mod Manager (such as mod installation, removal, and SMAPI installation) are executed through the durable operation engine.

### Operation Lifecycle
```text
+---------+      prepare()      +------------+
|  Draft  +-------------------->+  Prepared  |
+----+----+                     +-----+------+
     |                                |
     | cancel()                       | commit()
     v                                v
+----+----+                     +-----+------+
|Cancelled|                     |  Running   |
+---------+                     +-----+------+
                                      |
                                      | execute steps
                                      v
                                +-----+------+
                                | Committing |
                                +-----+------+
                                      |
                         +------------+------------+
                         |                         |
                         v                         v
                   +-----+------+            +-----+------+
                   | Succeeded  |            |Failed / Rec|
                   +------------+            +------------+
```

1. **`Draft`**: The archive is copied to immutable package storage, inspected, and staged. No mutations to live profile files have occurred. If the application crashes or exits, Draft operations can be safely discarded or cleaned up.
2. **`Prepared`**: The plan has been verified against dependencies and profile state. The plan is **immutable** once prepared.
3. **`Running`**: Step execution is actively modifying the filesystem.
4. **`Committing`**: The authoritative SQLite transaction is being applied.
5. **`Succeeded`**: Both filesystem changes and database records are fully reconciled.
6. **`RecoveryRequired`**: An error occurred during live modification that could not be automatically and safely rolled back, requiring user review.

---

## 2. Persisted Steps & Effects

### Steps Table (`operation_steps`)
Every operation records fine-grained steps to allow deterministic crash analysis and resumption:
- `operation_id`: Foreign key to `operations(id)`
- `step_index`: Execution sequence (e.g. 1 to 8)
- `step_kind`: Type of action (`RetainArtifact`, `InspectArchive`, `StageFiles`, `VerifyStagedContent`, `PrepareRecovery`, `PublishDeployment`, `CommitDatabase`, `CleanupStaging`)
- `state`: `Pending`, `Running`, `Completed`, `Failed`
- `payload_json`: Step-specific context
- `started_at`, `completed_at`, `error_json`

### Semantic Effects Table (`operation_effects`)
Authoritative database commits insert semantic change records in the same SQLite transaction:
- `ProfileComponentAdded`
- `ProfileComponentRemoved`
- `ProfileComponentVersionChanged`
- `ProfileComponentEnabled`
- `ProfileComponentDisabled`
- `SmapiRuntimeChanged`
- `ProfileCreated`
- `ProfileDeleted`

This enables the **Activity** feed and powers future "what changed since it worked?" diagnostic comparisons.

---

## 3. Resource-Scoped Locking

Rather than globally blocking the manager whenever any operation is pending or failed, operations declare affected resources in `operation_resources`:
- `operation_id`
- `resource_kind`: `GameInstallation`, `Profile`, `Artifact`
- `resource_id`: The specific resource UUID or hash
- `access_mode`: `Read` or `Write`

An in-memory lock coordinator serializes operations that require write access to the same resource. If Profile A requires recovery, operations on Profile B can proceed normally.

---

## 4. Stale-Plan Protection

Each profile tracks a monotonically increasing `revision: u64`:
1. When an operation is prepared, it captures `expected_profile_revision = profile.revision`.
2. Before committing, the operation engine verifies that `profile.revision == expected_profile_revision`.
3. If another operation changed the profile in the meantime, the commit is rejected with `OperationConflict`.
4. The user is prompted to refresh the preview rather than applying an outdated plan.

---

## 5. Artifact Retention & Cleanup Policy

- Source ZIP archives are copied to immutable storage (`packages/<sha256>.zip`) upon initial selection.
- If an operation fails during inspection or is cancelled in `Draft`, the unreferenced artifact file is eligible for conservative cleanup.
- Any artifact referenced by a completed deployment or historical operation is never deleted.
