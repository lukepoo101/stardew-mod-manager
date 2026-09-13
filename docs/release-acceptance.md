# Linux alpha release acceptance

Build artifacts are experimental until this checklist is completed for the exact commit. Record date, distro/architecture, game version, SMAPI archive hash, desktop session (Wayland/X11), result and sanitized evidence. Do not commit real game/mod archives or personal logs.

- [ ] Fresh clone installs with frozen pnpm lockfile and locked Cargo dependencies.
- [ ] Formatting, Clippy, Rust tests, TypeScript, frontend tests and native build pass.
- [ ] Dependency vulnerability and redistribution-license inventory reviewed.
- [ ] SMAPI pinned URL, digest, release/source and executable path verified against the actual downloaded release.
- [ ] Exact real installer tested for success, invalid path, permission failure and redirected input/output, with post-install artifacts checked.
- [ ] RPM installed on target Fedora; desktop-menu launch succeeds.
- [ ] Real mod inspected/installed; managed Mods path and bundled-mod behavior verified.
- [ ] Game reaches title screen; intended mod behavior confirmed in-game.
- [ ] Manager closes while game survives; reopening avoids duplicate launch and unsafe termination.
- [ ] Removal scope, dependency impact and repeat/restart recovery verified.
- [ ] Keyboard dialog behavior, scaling and multi-monitor positioning checked.
- [ ] Pre-release notes list tested configurations and remaining limitations; publish SHA-256 checksums with RPM.

None of these unchecked items is implied by a green synthetic test suite.
