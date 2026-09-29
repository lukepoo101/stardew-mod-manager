# Stardew Mod Manager Architecture

Stardew Mod Manager is a modular monolith. It is one desktop application built
from independently testable Rust crates with a single React shell, and the
dependency direction is enforced:

```text
manager-core
    ^
manager-app
    ^
manager-infra
    ^
apps/desktop/src-tauri
    ^
apps/desktop (React)
```

## Layers

### `manager-core`

Pure domain models and rules: game identity and inspection models, profiles,
package/component/deployment concepts, manifests, dependency evaluation,
operations and their state machine, launch/session models, health findings, and
SMAPI policy/value types.

There is no filesystem, process, environment, database, network or Tauri code in
this crate. A source-boundary test scans every file under
`crates/manager-core/src` with no allowlist.

### `manager-app`

Application orchestration and ports. It owns the bounded services (bootstrap,
games, profiles, packages, mods, operations, SMAPI, launch, diagnostics, health),
the durable operation engine and its recovery, the in-process resource
coordinator, application errors, and the Rust-owned IPC/read-model DTOs.

### `manager-infra`

Concrete adapters: SQLite persistence and persisted-data decoding, managed
filesystem deployment/staging, archive inspection and staging verification,
platform discovery, process launch and log reading, HTTP downloads, and the
cross-process file lock.

### Platform adapters

All host-specific behaviour lives under `crates/manager-infra/src/platform/`,
and `HostPlatform::for_host()` is the single place in the codebase that selects
by target operating system:

```text
platform/
  shared/    layouts, Steam VDF reader, deps.json reader, path-safe filesystem ops
  linux/     Steam client locations, process backend, log location, launch layout
  windows/   registry, Win32 process backend, log location, launch layout
  process.rs the process-lifecycle capability both backends implement
```

Everything that is data rather than an API call is compiled on every host, so a
Linux host can describe a Windows installation and explain why it cannot manage
it. Application services depend on the ports in `manager-app` and never learn
which host they run on; a service that needs platform behaviour asks the runtime
or process port for it rather than branching on the target.

### `apps/desktop/src-tauri`

Composition root and IPC adapter. Commands translate request arguments into
application-service calls and return DTOs or `ApiErrorDto`; they do not own
product rules. It also owns the single backend cache-invalidation event.

### `apps/desktop` (React)

Desktop shell and workflow presentation. Backend state lives in TanStack Query,
product decisions such as dependency, removal and launch safety stay in Rust, and
server-state synchronization is driven by one backend event.

## Frontend server state

The backend emits exactly one payload-free Tauri event,
`backend-state-changed`, after every state-changing command - on success and on
failure, because a failed command can still have durably changed state. One
frontend module turns it into `invalidateQueries()`; no mutation hook maintains
its own query-key dependency graph, and no feature code imports the Tauri event
API.

Polling is kept only where backend state changes without a command completing:
operation progress while a command is still running, and launch-session/process
observation.

## Operation engine

Install, removal and SMAPI setup are durable operations.

- A v2 operation persists a `Running` step before each live side effect and a
  `Completed` step only once evidence confirms it happened; compensation steps
  are persisted only when compensation actually becomes necessary.
- Every runtime state transition is validated by `manager-core`'s state machine
  and written through one lifecycle helper. The only exception is a
  tightly-scoped SQLite helper whose transition is valid by construction, such as
  the atomic `Committing -> Succeeded` inside an install/removal commit; those
  helpers assert the expected previous state.
- A prepared plan is frozen: `plan_json`, `plan_schema_version`, the operation
  kind and the expected profile revision are never rewritten underneath
  execution.
- Restart recovery is routed by provenance: migrated historical operations and
  plan-schema-v1 operations use isolated compatibility reconciliation, and
  plan-schema-v2 operations are recovered deterministically from persisted steps
  plus concrete filesystem and database evidence.

## Concurrency

Three mechanisms answer three different questions:

| Mechanism | Question |
| --- | --- |
| `operation_resources` (persisted) | What unresolved durable work owns this resource? |
| In-process resource coordinator | What is executing in this process right now? |
| `FileInstanceLock` | Is another application process using these files? |

Reads coexist; anything involving a write conflicts on the same resource
identity, and disjoint profiles or game installations proceed independently. The
in-process coordinator is deliberately in-process only: it is not a distributed
lock, and a conflicting request fails fast rather than queueing. The file lock
remains the cross-process guard and is re-entrant within one process, so two
disjoint operations no longer look like a second application instance to each
other.

## Persisted data

SQLite is the local state store. Published migrations are historical evidence and
are never renumbered, squashed or rewritten.

Historical persisted-data compatibility is deliberate and separate from the
runtime: published migrations (including migration 0007), the legacy database
upgrade tests, migrated historical-operation reconciliation, and plan-schema-v1
operation reconciliation all exist to open and safely reconcile older persisted
state. They are compatibility mechanisms, not alternate runtime architectures,
and no new operation is ever created through them.

Persisted semantic values are decoded strictly. A value the reader does not
recognise is a storage error carrying the table, column and raw value - it is
never quietly reinterpreted as some other valid domain value, and the offending
row is never rewritten. A deliberate `"unknown"` literal still decodes to a real
`Unknown` variant where the domain has one.

## Core invariants

- Profile is the canonical product concept; the physical
  `setups/<profile-id>/Mods` directory name is an infrastructure compatibility
  detail.
- Package artifact identity is content-addressed by SHA-256; acquisitions,
  components and deployments have separate identities.
- A bundle is one physical `ProfileDeployment` with one or more
  `ProfileComponent`s.
- Raw manifest source is evidence/provenance; the normalized `Manifest` is the
  semantic model.
- Prepared profile mutations carry an expected profile revision and fail rather
  than silently committing stale plans.
- Managed paths are derived from trusted IDs and relative paths, never from
  arbitrary persisted absolute paths.
- A process spawn is not launch verification; a session must establish mod
  loading from log evidence or stay unverified/unavailable/failed.
- Existing unmanaged Stardew installations are not silently adopted.
- Recovery never equates "could not read evidence" with "evidence absent".
- Package archives are validated against the Windows file namespace on every
  host, because the package is the same artifact wherever it is extracted.
- Two paths that a filesystem cannot distinguish are the same installation;
  comparison follows the host's path semantics rather than string equality.
- A process is terminated only when this session can prove the pid, its creation
  time and its image still identify the process it started.
