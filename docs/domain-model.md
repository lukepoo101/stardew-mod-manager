# Stardew Mod Manager — Domain Model

## 1. Identity & Value Types

All domain entities use strongly typed IDs rather than raw strings:

| Type | Underlying Representation | Purpose |
| :--- | :--- | :--- |
| `ArtifactHash` | SHA-256 Hex Digest | Canonical identity of an immutable `PackageArtifact` |
| `GameInstallationId` | UUID v4 | Identifies a detected/configured Stardew installation |
| `ProfileId` | UUID v4 | Identifies a distinct modding profile |
| `AcquisitionId` | UUID v4 | Identifies a package acquisition event |
| `PackageComponentId` | UUID v4 | Identifies an individual mod/pack inside a package |
| `DeploymentId` | UUID v4 | Identifies an artifact materialized into a profile |
| `ProfileComponentId` | UUID v4 | Identifies a component participating in a profile |
| `OperationId` | UUID v4 | Identifies a mutating or maintenance operation |
| `LaunchSessionId` | UUID v4 | Identifies a game launch instance |
| `FindingId` | UUID v4 | Identifies an observed health finding |
| `ModUniqueId` | String | The official SMAPI `UniqueID` (e.g. `Pathoschild.Automate`) |

> **Important**: `ModUniqueId` is an external SMAPI identity, strictly distinct from internal entity IDs.

---

## 2. Package & Deployment Tier

The mod management lifecycle separates immutable bytes, acquisition metadata, contained components, profile deployments, and active participation:

```text
+-------------------------------------------------------------+
|                      PackageArtifact                        |
|  hash: ArtifactHash, byte_size: u64, first_seen_at: Utc     |
+------------------------------+------------------------------+
                               |
            +------------------+------------------+
            | 1..*                                | 1..*
+-----------v-----------+             +-----------v-----------+
|      Acquisition      |             |   PackageComponent    |
|  source: LocalFile,   |             |  unique_id: ModUniqueId|
|  original_filename,   |             |  name, author, version|
|  acquired_at          |             |  parsed Manifest      |
+-----------------------+             |  raw_manifest: String |
                                      +-----------+-----------+
                                                  |
                                                  | 1
                                                  |
+-----------------------+             +-----------v-----------+
|   ProfileDeployment   | 1       1..*|   ProfileComponent    |
|  profile_id: ProfileId+------------->  deployment_id        |
|  artifact_id: Hash    |             |  package_component_id |
|  root_relative_path   |             |  enabled: bool        |
+-----------------------+             |  installed_reason     |
                                      +-----------------------+
```

### Aggregate Definitions

- **`PackageArtifact`**: The byte-level source archive saved in `packages/<sha256>.zip`.
- **`Acquisition`**: How the bytes were obtained (local file upload, URL download, provider API).
- **`PackageComponent`**: A single mod manifest located inside the archive, preserving both normalized manifest fields and the original raw text.
- **`ProfileDeployment`**: The extracted and placed artifact root inside a profile's isolated directory.
- **`ProfileComponent`**: The active presence of a component within a profile, tracking whether it is enabled and why it was installed (e.g., Direct user action vs. Dependency).

---

## 3. Profile & Game Context

### `Profile`
```text
Profile
  id: ProfileId
  game_installation_id: GameInstallationId
  name: String
  description: Option<String>
  revision: u64
  created_at: DateTime<Utc>
  updated_at: DateTime<Utc>
  state: ProfileState (Active, Archived, Corrupted)
```
- `revision` increments on every successful mutation. Operations verify `expected_profile_revision` to prevent applying stale plans.

### Context State
Separates the profile aggregate from selection state:
- **`AppContext`**:
  - `active_game_installation_id: Option<GameInstallationId>`
  - `onboarding_disposition: OnboardingDisposition` (`NotStarted`, `Skipped`, `Completed`)
- **`GameProfileContext`**:
  - `game_installation_id: GameInstallationId`
  - `active_profile_id: Option<ProfileId>`
  - `default_profile_id: Option<ProfileId>`
  - `last_active_profile_id: Option<ProfileId>`

---

## 4. Game Installation & Inspection

- **`GameInstallation` (Persistent Identity)**:
  - `id: GameInstallationId`
  - `canonical_root: PathBuf`
  - `operating_system: OperatingSystem` (`Linux`, `Windows`, `MacOS`)
  - `storefront: Storefront` (`Steam`, `Gog`, `Manual`, `Unknown`)
  - `management_mode: ManagementMode` (`Managed`, `ExternalUnmanaged`)
  - `created_at: DateTime<Utc>`

- **`GameInspection` (Read-Time Observation)**:
  - `installation_id: Option<GameInstallationId>`
  - `observed_game_version: Option<String>`
  - `observed_smapi_state: SmapiObservation`
  - `user_mod_state: UserModState`
  - `write_capability: WriteCapability`
  - `support_state: SupportState` (`SupportedFresh`, `SupportedManaged`, `ExistingModdedUnmanaged`, `UnsupportedPlatform`, `InvalidGameDirectory`, `Unreadable`, `Unwritable`, `Unknown`)
  - `evidence: Vec<InspectionEvidence>`
  - `inspected_at: DateTime<Utc>`

---

## 5. Dependency Graph

The canonical `DependencyGraph` models profile components and their relationships:
- **Nodes**: Profile components (`ModUniqueId`, `version`, `enabled`).
- **Edges**:
  - `Required`: Source requires target $\ge$ version.
  - `Optional`: Source enhances target if present.
  - `ContentPackFor`: Source is a content pack requiring target host mod.

The graph evaluates:
- Missing required dependencies
- Incompatible version constraints
- Reverse dependents (for safe removal planning)
- Duplicate mod IDs
- Dependency cycles
