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

## PR #317 migration status

The architecture above is the accepted destination, but PR #317 is a staged foundation rather than the final deletion of every MVP compatibility path.

Already migrated:
- first-class Profile identity/context;
- immutable package retention and package/component/deployment records;
- bounded `manager-app` services and repository/platform/runtime ports;
- application-layer install/remove and launch paths;
- modern SMAPI install bridge through `SmapiService`;
- persisted operation/resource/effect records and profile-revision stale-plan protection;
- generated TypeScript DTO bindings;
- routed desktop shell and feature boundaries;
- native Tauri file/folder dialogs and `reqwest` downloads.

Still transitional:
- `manager-core::use_cases` and `manager-core::install` retain legacy side effects behind an explicit CI allowlist while compatibility commands are retired;
- the operation engine does not yet persist/reconcile every execution step or provide the final in-process resource lock coordinator;
- the frontend compatibility router/query shims have not yet been replaced by React Router and TanStack Query;
- some legacy IPC commands and the old repository compatibility interface remain for staged cutover.

These exceptions are migration debt, not alternate architectural choices. ADR-0011, ADR-0013 and ADR-0014 document the completion gates explicitly.

## Core invariants

- Profile is the canonical product concept; the physical `setups/<profile-id>/Mods` directory name is an infrastructure compatibility detail.
- Package artifact identity is content-addressed by SHA-256; acquisitions, components and deployments have separate identities.
- A bundle is one physical `ProfileDeployment` with one or more `ProfileComponent`s.
- Raw manifest source is evidence/provenance; normalized `Manifest` is the semantic model.
- Prepared profile mutations carry an expected profile revision and must fail rather than silently commit stale plans.
- Managed paths are derived from trusted IDs and relative paths, never arbitrary persisted absolute mutation targets.
- A process spawn is not launch verification; session evidence must establish mod loading or the session remains unverified/unavailable/failed.
- Existing unmanaged Stardew installations are not silently adopted.

## Follow-up completion gates

The foundation migration is complete when:
1. the legacy `CoreUseCases`/`AppSnapshot`/`StateRepository` compatibility stack is removed;
2. the `manager-core` source-boundary allowlist is empty;
3. operation state transitions, execution steps and resource locks are centrally enforced and restart-reconciled;
4. the frontend uses React Router + TanStack Query with backend event invalidation;
5. the remaining IPC surface returns structured API errors consistently.
