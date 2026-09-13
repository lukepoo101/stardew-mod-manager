# ADR-0011: Modular Monolith Application Layer Architecture

## Status
Accepted

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
   - Defines unified application error handling (`AppError`) and maps errors to `ApiErrorDto`.
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

## Consequences
### Positive
- Strict separation of pure domain business logic from infrastructure I/O.
- High testability: pure domain logic is tested without filesystem mocks; application use cases are tested with in-memory port fakes; infrastructure adapters are tested with integration contract suites.
- Prevents god-object accumulation: use cases are organized into bounded application services (`BootstrapService`, `GamesService`, `ProfilesService`, `ModsService`, `OperationsService`, `SmapiService`, `LaunchService`, `DiagnosticsService`, `HealthService`).
- Keeps the system as a single deployable desktop executable without daemon or network process overhead.

### Negative
- Requires explicit port and adapter definitions rather than direct ad-hoc calls from commands to database/filesystem.
- Slightly more crate boilerplate and dependency management across the workspace.
