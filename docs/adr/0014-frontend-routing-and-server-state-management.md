# ADR-0014: Frontend Routing and Server-State Management Architecture

## Status
Accepted target — library cutover pending

## Date
2026-09-13

## Context
The MVP frontend implemented navigation via imperative `if`/`else` branches in `App.tsx` evaluated against a single monolithic `AppSnapshot` object returned by `get_app_snapshot()`. Every user mutation triggered a full backend snapshot reload (`loadSnapshot()`).

This approach caused several fundamental limitations:
1. No bookmarkable, deep-linkable, or browser-history-compatible routes.
2. Mod inspection review, mod removal confirmation, log viewing, and recovery screens competed for single modal/conditional slots.
3. Every button press caused a heavyweight query reload across games, setup, SMAPI status, all mods, active operations, and active sessions.
4. Transient UI states (filters, open modals) were frequently blown away during whole-page re-renders.

## Decision
Adopt a modern, decoupled desktop application frontend architecture utilizing **React Router** for view navigation and **TanStack Query** for server-state caching and targeted invalidation:

### 1. Hash-Based Application Router (`createHashRouter`)
Desktop Tauri webviews run over custom local protocols (such as `tauri://localhost` or file protocols) where URL hash navigation (`createHashRouter`) prevents asset-path mismatches. Routes are declared using standard relative paths:
- `/onboarding`: Multi-step guided wizard (`/onboarding/game`, `/onboarding/review`, `/onboarding/install-runtime`, `/onboarding/complete`).
- `/app/overview`: Playthrough dashboard with active game/profile, health summary, and quick actions.
- `/app/mods`: Mod inventory table with toolbar, filtering, and sorting.
- `/app/mods/:profileComponentId`: Deep-linkable mod details panel.
- `/app/mods/install/:operationId`: Routed, durable install review and progress view.
- `/app/profiles`: Profile management (list, create clean profile, switch active profile).
- `/app/profiles/:profileId`: Profile details and configuration.
- `/app/diagnostics`: Session history and diagnostics correlation.
- `/app/diagnostics/sessions/:sessionId`: Structured SMAPI log and finding inspection.
- `/app/activity`: Audit log of durable operations and change effects.
- `/app/activity/:operationId`: Specific operation detail and recovery view.
- `/app/settings/*`: Dedicated views for Game Installations, Runtime, Storage, Appearance, and Advanced.

### 2. TanStack Query for Backend Server-State
- Rust backend state is cached and queried using TanStack Query hooks (`useQuery`, `useMutation`).
- Fine-grained query keys prevent unnecessary refetches (e.g., `["profile-mods", profileId]`, `["operation", operationId]`, `["smapi-status", gameId]`).
- Mutations invalidate only affected query keys upon completion rather than reloading the entire application.

### 3. Low-Frequency Backend Event Bridge
Tauri backend emits low-frequency state-change events:
- `operation_changed`
- `profile_changed`
- `inventory_changed`
- `session_changed`
- `health_changed`

A global event listener subscribes once at the root and triggers targeted query invalidation in TanStack Query.

### 4. Clear State Ownership
- **Backend state**: Owned exclusively by TanStack Query.
- **Route state**: Owned by React Router (active view, selected mod, active install review).
- **Local UI state**: Owned by React component state (`useState`, form inputs, drawer open/close).

## Transitional implementation status

PR #317 introduces the routed application shell, feature boundaries and query-key-oriented hooks, but it currently uses local compatibility implementations in `shared/router` and `shared/api/query`. Those shims are a migration aid only; they do **not** satisfy the library decision in this ADR and should not become a long-term framework maintained by the project.

The React Router (`createHashRouter`) and TanStack Query package cutover, durable operation/detail routes, and Tauri event-driven query invalidation remain explicit follow-up work. New frontend architecture should continue to preserve the state-ownership split above so the eventual library swap is mechanical rather than another product rewrite.

## Consequences
### Positive
- The target architecture gives stable, linkable navigation across operations, diagnostics, and settings.
- Fine-grained backend queries avoid rebuilding the former monolithic `AppSnapshot` dependency pattern.
- The transitional shell already establishes feature boundaries and route-oriented UI composition.
- The final library cutover provides established caching, invalidation, history and error-handling behavior instead of growing bespoke infrastructure.

### Negative
- Requires maintaining query key factories and mutation invalidation mapping.
- Additional client dependencies (`react-router`, `@tanstack/react-query`).
- Until the package cutover lands, the local compatibility router/query shims remain temporary technical debt and must not be described as React Router or TanStack Query themselves.
