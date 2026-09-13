# ADR-0012: Profile, Package, Component, and Deployment Domain Model

## Status
Accepted

## Date
2026-09-13

## Context
The MVP assumed a single default `Setup` per game installation and merged the identity of downloaded archives, mod manifests, and active filesystem folders. Specifically:
- `InstalledMod.package_id` stored a SHA-256 hash without guaranteeing that the source package archive was authoritatively retained on disk.
- A single multi-mod ZIP archive could contain multiple SMAPI mods, but their provenance and bundle relationship were tracked ad-hoc through file inventories.
- Mod removal required frontend inspection of `relative_target_path` and `package_id` to infer bundle companions.
- Freshness, managed ownership, and game existence were coupled as booleans on `GameInstallation`.

To support profiles, mod updates, exact rollbacks, collections, multiplayer comparison, and unambiguous provenance, the domain requires distinct concepts for immutable archives, acquisition provenance, contained components, physical profile deployments, and active component participation.

## Decision
Establish the canonical five-tier package and deployment model:

1. **`PackageArtifact`**:
   - Immutable source bytes stored in content-addressed storage (`packages/<sha256>.zip`).
   - Identified by its SHA-256 `ArtifactHash`.
   - Guaranteed to exist on disk for all newly acquired packages.

2. **`Acquisition`**:
   - Records how bytes entered the system (`LocalFile`, `DirectUrl`, `Provider`, `ManualReference`).
   - Stores original filename, acquisition timestamp, expected hash, and provider-neutral source metadata.
   - Distinct from SMAPI manifest `UpdateKeys` (which represent upstream update locations, not observed provenance).

3. **`PackageComponent`**:
   - A single SMAPI mod or content pack discovered inside a `PackageArtifact`.
   - Identified by `PackageComponentId` (UUID).
   - Contains normalized `Manifest` (Name, Author, Version, `ModUniqueId`, Dependencies, `MinimumApiVersion`, `MinimumGameVersion`, `UpdateKeys`) alongside `raw_manifest: String`.

4. **`ProfileDeployment`**:
   - Physical materialization of a `PackageArtifact` into a specific `Profile`.
   - Identified by `DeploymentId` (UUID).
   - Tracks root relative deployment path (`root_relative_path`), installed timestamp, and deployment state (`Present`, `Disabled`, `Missing`, `ExternallyModified`, `Quarantined`).

5. **`ProfileComponent`**:
   - Participation of a `PackageComponent` in a `Profile` via a `ProfileDeployment`.
   - Identified by `ProfileComponentId` (UUID).
   - Tracks enabled state, installation reason (`Direct`, `Dependency`, `BundleCompanion`), and links to the parent deployment.

### Separation of Profile Identity and Context
- `Profile` (`id: ProfileId`, `game_installation_id: GameInstallationId`, `name: String`, `description: Option<String>`, `revision: u64`, `created_at`, `updated_at`, `state`) replaces `Setup`.
- Active and default profile selections are separated into `AppContext` and `GameProfileContext`.
- `profiles.revision` increments on every successful profile mutation to detect and reject stale operations.
- Filesystem layout (`setups/<profile-id>/Mods`) is an infrastructure detail managed by `AppPaths`, keeping `manager-core` free of filesystem paths.

### Separation of Game Identity and Inspection
- `GameInstallation` stores persistent identity (`id: GameInstallationId`, `canonical_root`, `operating_system`, `storefront`, `management_mode`, `created_at`).
- `GameInspection` provides read-time observation (`observed_game_version`, `observed_smapi_state`, `user_mod_state`, `write_capability`, `support_state`, `evidence[]`, `inspected_at`).

## Consequences
### Positive
- Authoritative retention ensures exact rollback, re-installation, and offline bundle reproduction without relying on user download folders.
- Clear separation between multi-mod packages and individual mod components solves bundle installation and removal cleanly in the backend.
- Pure domain model without filesystem assumptions.
- Profile revisions prevent race conditions and stale operation commits.

### Negative
- Local disk usage increases slightly because source archives are retained under `packages/` until garbage-collected.
- Increased schema and mapping complexity compared to single-table MVP records.
