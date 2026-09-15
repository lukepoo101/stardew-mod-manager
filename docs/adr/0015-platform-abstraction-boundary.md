# ADR-0015: Platform Abstraction Boundary and Native Integration

## Status
Accepted

## Date
2026-09-13

## Context
The MVP made platform-specific Linux assumptions across several layers:
1. `AppPaths` relied on hardcoded `$HOME` and XDG environment variables.
2. Steam discovery searched Linux-specific filesystem roots (`.local/share/Steam`, `.steam/root`, Flatpak directories).
3. Game validation checked ELF binary magic bytes and executed Linux write probes.
4. SMAPI archive downloads executed external `curl` process commands via `std::process::Command`.
5. Mod file picking executed external `zenity` commands with heuristic fallback parsing for `~/Downloads`.
6. Custom random ID generation read directly from `/dev/urandom`.

These assumptions made adding future Windows or macOS support difficult and introduced security risks through process invocations.

## Decision
Establish clear platform abstraction boundaries in `manager-infra` and replace external tool dependencies:

### 1. Platform-Aware Directory Resolution
Replace hardcoded path resolution with a cross-platform directory provider using the Rust `directories` crate. Provide standard semantic accessors:
- `data_dir`
- `cache_dir`
- `state_db`
- `packages_dir`
- `profile_mods(profile_id)`
- `profile_staging(profile_id, operation_id)`
- `profile_recovery(profile_id, operation_id)`
- `smapi_cache`
- `logs`

Preserve `AppPaths::new(data_dir, cache_dir)` for hermetic automated tests.

### 2. Concrete Discovery and Inspection Ports
Define domain-agnostic ports in `manager-app`:
- `GameDiscoveryPort`: `discover() -> Vec<GameCandidate>`
- `GameInstallationInspectorPort`: `inspect(path) -> GameInspection`

Implement current Linux Steam discovery and Linux binary inspection under `crates/manager-infra/src/platform/linux/`. Future Windows/macOS implementations will implement the same ports without touching core domain models.

### 3. In-Process Native HTTP Transfers
Remove all usage of external `curl`. Implement a dedicated, secure HTTP downloader using `reqwest` in `manager-infra`:
- Downloads stream to an isolated `.tmp` file.
- Calculates SHA-256 hash progressively during streaming.
- Verifies hash against release policy before promoting to cache.
- Invokes `sync_all` (fsync) and atomically renames to the target path.
- Supports progress tracking and cancellation hooks.

### 4. Native Dialogs via Tauri Plugin
Remove all usage of external `zenity` and custom path guessing. Integrate the official `@tauri-apps/plugin-dialog` in the webview and `tauri-plugin-dialog` in the native shell. Open dialogs directly from the native desktop environment and pass the selected canonical path into the backend for validation.

### 5. Standard Cryptographic IDs
Replace the custom `/dev/urandom` reader with standard Rust `uuid::Uuid::new_v4()`.

## Consequences
### Positive
- Removes external process execution vulnerabilities (`curl`, `zenity`).
- Makes the application cross-platform ready: domain and application layers have zero Linux-specific assumptions.
- Improves user experience by using native OS file selection dialogs.
- Clean separation of platform-specific code into dedicated infra modules.

### Negative
- Adds small binary size overhead for `reqwest` and `tauri-plugin-dialog`.
