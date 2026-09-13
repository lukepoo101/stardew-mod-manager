# ADR-0001: Desktop Application Form Factor

## Status
Accepted

## Date
2026-09-12

## Context
Stardew Valley mod management requires direct access to the local filesystem (Steam library paths, game installation folders, mod directories), invocation of native child processes (SMAPI installer, game executable), cross-process advisory locking (`fs2`), and real-time monitoring of local log files (`SMAPI-latest.txt`). A web-only application cannot safely or directly perform these operations without complex browser extension permissions, local helper daemons, or insecure socket bridges.

## Decision
Build the application as a native desktop application targeting Linux (with Fedora/RPM as the primary development target) and cross-platform desktop operating systems using Tauri 2.

## Consequences
### Positive
- Direct, unmediated access to local filesystem APIs and Steam directories.
- Native process spawning with custom process group isolation (`setpgid(0)`) and supervisory thread zombie reaping.
- Cross-process advisory file locking via OS primitives (`flock`/`fcntl`) to prevent concurrent manager instances or file corruption.
- Small binary footprint and low memory footprint compared to Electron.
- Native RPM packaging and desktop integration (`.desktop` entry, mime icons).

### Negative
- Platform-specific system dependencies (GTK3, WebKit2GTK on Linux).
- Cross-platform packaging requires distinct bundle configurations per OS.

## Alternatives Considered
- **Web App + Native Daemon**: Discarded due to installation friction and security surface area of local HTTP/WebSocket endpoints.
- **Electron**: Discarded due to bloated memory usage (~200MB+ baseline vs ~30MB for Tauri) and large distribution packages.
