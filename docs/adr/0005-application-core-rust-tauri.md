# ADR-0005: Application Core and Desktop Shell - Rust and Tauri 2

## Status
Accepted

## Date
2026-09-12

## Context
A mod manager interacts directly with system resources: reading ZIP archives, extracting files, parsing JSON manifests with comments, executing external installers, holding advisory locks, reading structured logs, and spawning detached child processes. The core logic must be safe, free from data races, performant, and securely isolated from web vulnerabilities.

## Decision
Implement the application core (`crates/manager-core`) and system infrastructure (`crates/manager-infra`) as pure Rust crates, integrated with the Tauri 2 desktop shell (`apps/desktop/src-tauri`). Maintain strict capability boundaries in Tauri: the frontend communicates exclusively over explicit IPC invoke commands (`inspect_game`, `install_smapi`, `commit_mod_install`, etc.) without broad filesystem or shell permissions exposed to the webview.

## Consequences
### Positive
- Memory safety and concurrency safety guaranteed at compile time.
- Clear architectural separation: pure domain logic and validation in `manager-core`, OS and filesystem adapters in `manager-infra`, IPC bindings in `src-tauri`.
- Tauri 2 capability model enforces the principle of least privilege, neutralizing XSS/injection risks.
- Extremely low memory footprint and native Linux GTK3/WebKit2GTK window integration.

### Negative
- Rust compilation times during full clean rebuilds.
- Dual-language stack (Rust + TypeScript) requires IPC type synchronization.

## Alternatives Considered
- **Electron (Node.js/C++)**: Higher memory overhead and riskier security model if nodeIntegration is mishandled.
- **Pure Rust GUI (Iced/Slint/egui)**: GUI ecosystems in Rust are evolving, but lack the mature ecosystem of accessible web components, styling libraries, and responsive layout systems found in React and Tailwind.
