# ADR-0011: Modular Monolith Application Layer Architecture

## Status
Accepted — application-layer boundary migration complete (operation-engine work tracked separately in ADR-0013)

## Date
2026-09-13

## Context
The initial Linux MVP implemented application logic primarily in `manager-core::use_cases::CoreUseCases`. As the application grew to support game detection, SMAPI installation, archive inspection, mod installation/removal, process launching, session verification, recovery, and diagnostics, `CoreUseCases` became a monolithic service exceeding 30,000 bytes. Furthermore, `manager-core` acquired direct dependencies on `std::fs` (e.g., probe writes, ELF header checks, staged content hash verification), blurring the boundary between pure domain logic and infrastructure side effects.

To support future capabilities—such as profiles, mod updates, remote providers (Nexus, CurseForge), collections, multiplayer synchronization, compatibility intelligence, and diagnostic workflows—the application requires bounded architectural responsibilities without introducing unnecessary operational complexity (such as microservices, remote daemons, or event sourcing).

## Decision
Adopt a clean **modular monolith** with three Rust crates and explicit dependency direction:

```text
manager-core (pure domain rules and models)
    ^
manager-app (use cases, application services, durable operations, query/read models, ports)
    ^
manager-infra (concrete adapters: SQLite, filesystem, reqwest HTTP, platform discovery)
    ^
src-tauri (composition root, thin async commands, event bridge)
```

### Layer Responsibilities

1. **`manager-core`**:
   - Contains pure domain rules, models, value types, and algorithms.
   - Zero side effects: no `std::fs`, no `std::process`, no `std::env`, no `rusqlite`, no `tauri`, no `reqwest`.
   - Paths may be manipulated as value objects, but no filesystem reads, writes, or probe checks are performed.
   - Enforced by architectural CI checks.

2. **`manager-app`**:
   - Owns application orchestration, use cases, durable operation workflows, and query read-models.
   - Defines abstract ports (traits) for repositories, storage, discovery, runtime installation, game launching, and system clock.
   - Defines unified application error handling (`AppError`) and converts it to `ApiErrorDto` at the IPC boundary, preserving code, category, summary, technical details, context, recoverability and operation id.
   - Depends only on `manager-core` and general standard/utility libraries; has no direct dependency on SQLite, Tauri, or platform-specific syscalls.

3. **`manager-infra`**:
   - Implements ports declared by `manager-app`.
   - Owns file-based SQLite persistence, migrations, and atomic mutation transactions.
   - Implements archive inspection, extraction, staging, and content hashing.
   - Implements OS and platform adapters (Linux Steam discovery, ELF validation).
   - Implements network transfers using `reqwest` with checksum verification.

4. **`src-tauri`**:
   - Serves strictly as the application composition root.
   - Instantiates concrete infrastructure adapters and registers application services.
   - Exposes thin, asynchronous IPC command handlers that deserialize inputs, invoke application services, and serialize generated DTOs.
   - Emits low-frequency state-change events for frontend cache invalidation.

## Implementation status

The application-layer boundary migration described by this ADR is complete.

- `manager-core` is pure: no `std::fs`, `std::process` or `std::env` anywhere under `crates/manager-core/src`. The source-boundary test scans every file with no allowlist, so the invariant is literal rather than aspirational.
- `manager-core::use_cases` (`CoreUseCases`, `AppSnapshot`) and the legacy `manager-core::ports::StateRepository`, `PackageStore`, `SmapiInstaller`, `GameLauncher` and `SessionLogReader` traits are deleted. `manager-core::ports::InstanceLock` remains because modern `manager-app` services genuinely consume it; all other external effects are owned by bounded `manager-app` ports.
- `manager-core::install` retains only pure plan, inventory, manifest-composition and relative-path validation types. Filesystem verification lives in the `manager-infra` staged-content verifier, archive inspection/extraction in the archive adapter, and publication in the deployment adapter.
- The composition root instantiates concrete `manager-infra` adapters once and wires them into the bounded `manager-app` services (`BootstrapService`, `GamesService`, `ProfilesService`, `PackagesService`, `ModsService`, `OperationsService`, `SmapiService`, `LaunchService`, `DiagnosticsService`, `HealthService`).
- The Tauri command surface registers one intentional command per frontend product action; the compatibility aliases and the commands used only by the deleted frontend compatibility client are removed.
- The frontend consumes generated Rust DTOs through a single IPC client; the manually duplicated `lib/backend` model is deleted. `shared/api/invoke.ts` is the only call site of the Tauri invoke API, and it normalizes every rejection into an `ApiClientError` carrying the Rust-generated `ApiErrorDto`.

What this ADR does **not** claim: the operation engine still lacks the persisted-step execution and resource-lock coordinator described by ADR-0013. That remains separately tracked follow-up work. The IPC error boundary is no longer outstanding: since ADR-0016's typed error contract shipped, every product command returns `IpcResult<T>` backed by the generated `ApiErrorDto`, and the frontend normalizes rejections into a single `ApiClientError`.

Statements such as “manager-core has zero side effects” now describe the shipped executable, not merely the target.

## Consequences
### Positive
- Strict separation of pure domain business logic from infrastructure I/O once the migration completes.
- High testability: pure domain logic is tested without filesystem mocks; application use cases are tested with in-memory port fakes; infrastructure adapters are tested with integration contract suites.
- Prevents god-object accumulation: use cases are organized into bounded application services (`BootstrapService`, `GamesService`, `ProfilesService`, `ModsService`, `OperationsService`, `SmapiService`, `LaunchService`, `DiagnosticsService`, `HealthService`).
- Keeps the system as a single deployable desktop executable without daemon or network process overhead.

### Negative
- Requires explicit port and adapter definitions rather than direct ad-hoc calls from commands to database/filesystem.
- Slightly more crate boilerplate and dependency management across the workspace.
- The boundary is enforced by CI guardrails (crate dependency checks, the `manager-core` source-boundary scan, the `manager-app` infrastructure-IO scan and the frontend command-registration test) rather than by the type system alone. Historical persisted-data compatibility is preserved independently of this boundary: migrations, legacy-row decoding and the reconciliation of migrated interrupted operations stay in `manager-infra`/`manager-app` where the effects belong.
