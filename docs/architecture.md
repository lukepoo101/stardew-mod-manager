# Stardew Mod Manager — Architectural Guide

## 1. System Overview

Stardew Mod Manager is designed as a **modular monolith** desktop application. It combines a fast, responsive native desktop shell with offline-first, crash-resilient mod management for Stardew Valley and SMAPI.

```text
                         +---------------------+
                         |     React UI        |
                         | routes + queries    |
                         +----------+----------+
                                    | typed IPC DTOs
                         +----------v----------+
                         |    Tauri adapter    |
                         | commands + events   |
                         +----------+----------+
                                    |
                         +----------v----------+
                         |    manager-app      |
                         | application layer   |
                         | use cases / queries |
                         +-------+-------+-----+
                                 |       | ports
                   +-------------v-+   +-v----------------+
                   | manager-core  |   |  manager-infra   |
                   | pure domain   |   | SQLite/FS/OS/HTTP|
                   | rules/models  |   | process/archive  |
                   +---------------+   +------------------+
```

## 2. Bounded Crates

### `manager-core`
- **Responsibility**: Pure domain models, immutable value types, business rules, and algorithms.
- **Constraints**: Absolutely no I/O, no network, no database, no OS-specific syscalls, and no side effects.
- **Forbidden dependencies**: `std::fs`, `std::process`, `std::env`, `rusqlite`, `tauri`, `reqwest`.
- **Primary contents**:
  - Strongly-typed IDs (`ProfileId`, `GameInstallationId`, `ArtifactHash`, `ModUniqueId`, etc.)
  - Domain aggregates: `Profile`, `GameInstallation`, `PackageArtifact`, `Acquisition`, `PackageComponent`, `ProfileDeployment`, `ProfileComponent`
  - Normalized SMAPI manifest parsing with `raw_manifest` retention
  - Canonical `DependencyGraph` and compatibility evaluation
  - State machine models for operations, launch sessions, and health findings

### `manager-app`
- **Responsibility**: Use case orchestration, query read-models, operation engine execution, and abstract port declarations.
- **Constraints**: No direct coupling to concrete database engines (rusqlite), Tauri shell APIs, or platform syscalls.
- **Primary contents**:
  - Ports: Repositories, artifact storage, staging, process launching, HTTP transport, clock
  - Application services: `BootstrapService`, `GamesService`, `ProfilesService`, `PackagesService`, `ModsService`, `OperationsService`, `SmapiService`, `LaunchService`, `DiagnosticsService`, `HealthService`
  - Read-model queries: `ProfileOverview`, `ModListItem`, `ModDetails`, `SmapiStatus`, `HealthSummary`
  - Unified error handling: `AppError` and conversion to `ApiErrorDto`
  - IPC Data Transfer Objects with `ts-rs` binding derivation

### `manager-infra`
- **Responsibility**: Concrete implementations of ports declared in `manager-app`.
- **Primary contents**:
  - SQLite repositories with file-based schema migrations (`0001`, `0002`, `0003`)
  - `AtomicMutationStore` for atomic multi-entity transactions
  - Content-addressed package artifact store (`packages/<sha256>.zip`)
  - Secure archive inspection, extraction, staging, and content verification
  - Cross-platform directory resolution (`AppPaths`)
  - Platform adapters (`platform/linux/` for Steam discovery and ELF validation)
  - Native HTTP streaming downloader using `reqwest` with checksum verification
  - Process management for game launching and SMAPI installation

### `src-tauri`
- **Responsibility**: Application entry point, dependency injection / composition root, and IPC commands.
- **Primary contents**:
  - Instantiates concrete infra adapters and configures `AppServices`
  - Thin asynchronous command handlers (`commands/`)
  - Emits low-frequency state-change events (`operation_changed`, `profile_changed`, etc.)
  - Manages window geometry persistence and native desktop integrations

## 3. Data Flow & Transaction Boundaries

### Query Flow
```text
React Component -> TanStack Query hook -> Backend client -> Tauri invoke
  -> Tauri command -> manager-app Query -> ReadModelRepository (SQL join) -> DTO -> React
```

### Mutation Flow (Prepare -> Preview -> Commit)
```text
1. Prepare:
   User Action -> Tauri command -> Application Service (e.g. ModsService::prepare_install)
     -> Copy/Hash to ArtifactStore -> Stage files -> Verify staging
     -> Persist Operation in 'Draft' -> Return OperationPreviewDto

2. Preview:
   Frontend displays proposed changes, affected components, dependencies, and filesystem impact.

3. Commit:
   User confirms -> Tauri command -> Application Service (e.g. ModsService::commit_install)
     -> Validate expected profile revision
     -> Transition Operation to 'Running'
     -> Publish deployment to profile Mods folder
     -> Execute AtomicMutationStore::commit_install (writes deployment, components, effects, Succeeded)
     -> Clean up staging
     -> Emit 'operation_changed' event -> Frontend invalidates TanStack Query keys
```

## 4. Key Architectural Invariants
1. **Managed Filesystem Isolation**: Mods are physically deployed into profile-specific directories (`setups/<profile-id>/Mods`) and loaded via SMAPI `--mods-path`. The vanilla game directory is never polluted.
2. **Deterministic Path Derivation**: Target directories, staging trees, and recovery folders are derived exclusively by trusted infrastructure services using internal IDs. Frontend input cannot specify arbitrary destinations.
3. **Immutability of Prepared Operations**: Once an operation reaches `Prepared`, its plan is fixed. If the underlying profile revision changes before commit, the operation fails revalidation and must be refreshed.
4. **Authoritative Package Retention**: Every installed mod component references a permanently retained `PackageArtifact`, ensuring exact rollback and reproducibility.
