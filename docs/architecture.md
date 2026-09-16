# Stardew Mod Manager Architecture

Stardew Mod Manager is evolving from the original Linux MVP into a modular monolith. The target dependency direction is:

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

## Architectural target

### `manager-core`
Pure domain models and rules: game identity/inspection models, profiles, package/component/deployment concepts, manifests, dependency evaluation, operations, launch/session models, health findings and SMAPI policy/value types.

The completed migration has no filesystem, process, environment, database, network or Tauri side effects in this crate.

### `manager-app`
Application orchestration and ports. It owns bounded services such as bootstrap, games, profiles, packages, mods, operations, SMAPI, launch, diagnostics and health, plus Rust-owned IPC DTOs/read models.

### `manager-infra`
Concrete adapters for SQLite, managed filesystem storage/deployment, archive inspection and staging, platform discovery, process launch/log reading and HTTP downloads.

### Tauri
Composition root and IPC adapter. Commands should translate request DTOs into application-service calls and return DTOs/errors without owning product rules.

### React
Desktop shell and workflow presentation. Backend state is queried through the IPC client; product decisions such as dependency/removal/launch safety remain in Rust.

## Migration status

The architecture above is the accepted destination. The application-layer boundary migration is complete: the MVP compatibility stack has been removed and every production path runs on the single `manager-app` service graph.

Completed:
- first-class Profile identity/context;
- immutable package retention and package/component/deployment records;
- bounded `manager-app` services and repository/platform/runtime ports;
- application-layer install/remove and launch paths;
- modern SMAPI install bridge through `SmapiService`;
- persisted operation/resource/effect records and profile-revision stale-plan protection;
- generated TypeScript DTO bindings;
- routed desktop shell and feature boundaries;
- React Router + TanStack Query for frontend routing and server state;
- native Tauri file/folder dialogs and `reqwest` downloads.

The compatibility stack has been deleted rather than retained:

- `manager-core::use_cases` (`CoreUseCases`, `AppSnapshot`) is gone, and `manager-core` is pure domain code with no source-boundary allowlist: the boundary test scans every file under `crates/manager-core/src` with no exceptions.
- `manager-core::install` keeps only pure plan/inventory/validation types; filesystem verification moved to the `manager-infra` staged-content verifier.
- The legacy `StateRepository`/`PackageStore`/`SmapiInstaller`/`GameLauncher`/`SessionLogReader` core ports are gone. `InstanceLock` remains because modern services still consume it; every other external effect is owned by a bounded `manager-app` port.
- The frontend `lib/backend` compatibility model (manually duplicated `AppSnapshot`/`Setup`/`InstalledMod`/`InstallPlan`/`Operation`/`LaunchSession` interfaces and the stateful `MockBackend`) is gone. Generated Rust DTOs are the only IPC contract, and `shared/api/client.ts` is the only IPC client.
- `tauri::generate_handler!` registers one intentional command per product action. The legacy aliases (`list_games`, `inspect_game_path`, `accept_game`, `select_profile`, `list_mods`, `prepare_install`, `commit_operation`, `get_operation`, `list_operations`, `get_diagnostics`, `pick_mod_file`, `pick_game_directory`) and the commands that existed only for the removed compatibility client are deleted.

Still transitional (separate, explicitly deferred work):

- the operation engine does not yet persist/reconcile every execution step or provide the final in-process resource lock coordinator (ADR-0013);
- the remaining IPC surface does not yet return structured API errors consistently;
- the backend does not yet emit low-frequency event-driven cache invalidation.

Historical persisted-data compatibility is deliberately preserved: all published migrations (including migration 0007), the modern reconciliation of migrated interrupted operations, and the legacy v1 database upgrade tests remain in place.

## Core invariants

- Profile is the canonical product concept; the physical `setups/<profile-id>/Mods` directory name is an infrastructure compatibility detail.
- Package artifact identity is content-addressed by SHA-256; acquisitions, components and deployments have separate identities.
- A bundle is one physical `ProfileDeployment` with one or more `ProfileComponent`s.
- Raw manifest source is evidence/provenance; normalized `Manifest` is the semantic model.
- Prepared profile mutations carry an expected profile revision and must fail rather than silently commit stale plans.
- Managed paths are derived from trusted IDs and relative paths, never arbitrary persisted absolute mutation targets.
- A process spawn is not launch verification; session evidence must establish mod loading or the session remains unverified/unavailable/failed.
- Existing unmanaged Stardew installations are not silently adopted.

## Remaining completion gates

The application-layer boundary migration is complete. The separately tracked follow-up work is:

1. operation state transitions, execution steps and resource locks are centrally enforced and restart-reconciled (ADR-0013);
2. the remaining IPC surface returns structured API errors consistently;
3. the backend emits low-frequency event-driven cache invalidation for the frontend query cache.
