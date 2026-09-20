# Release acceptance checklists

Build artifacts are experimental until the relevant checklist is completed for the exact commit. Record date, platform and architecture, game version, SMAPI archive hash, result and sanitized evidence. Do not commit real game, mod archives or personal logs.

Every item below is manual. None of them is implied by a green synthetic test suite, and a passing CI run is not acceptance.

## Windows acceptance

Run on **Windows 10 22H2 x64** and **Windows 11 x64**.

### Discovery and inspection

- [ ] Steam in its default location is discovered.
- [ ] Steam in a custom location (registry `SteamPath`) is discovered.
- [ ] Stardew Valley in the primary library is discovered.
- [ ] Stardew Valley in a secondary library on another drive is discovered.
- [ ] Manual folder selection works for a game outside Steam.
- [ ] A path containing spaces is accepted.
- [ ] A path containing non-ASCII characters is accepted.
- [ ] A mixed-case path is accepted and does not create a duplicate installation.
- [ ] A junction or aliased path does not create a duplicate installation.
- [ ] A folder missing `Stardew Valley.exe` is rejected with a clear reason.
- [ ] An incomplete installation is rejected with a clear reason.
- [ ] A read-only or access-denied installation is reported as unwritable.
- [ ] A Proton or Wine prefix selected on Windows is reported as a Linux installation and refused.

### Mods and storage

- [ ] A real mod ZIP is inspected, previewed and installed.
- [ ] The mod loads in-game from the profile's Mods directory.
- [ ] The mod is removed and disappears from the game directory.
- [ ] A bundle archive installs every component and removes them together.
- [ ] Dependency checks report a missing dependency before installation.
- [ ] A profile is created, switched and archived.
- [ ] A ZIP containing `Mod/config.json` and `Mod/Config.json` is rejected before extraction.
- [ ] A ZIP containing `CON`, `NUL`, `AUX`, `COM1` or `LPT1` entries is rejected.
- [ ] A ZIP containing a colon (alternate data stream) entry is rejected.
- [ ] A ZIP containing `../` traversal is rejected.
- [ ] A very deep ZIP is either installed correctly or refused with a clear message.
- [ ] Installing while a target file is locked produces a bounded retry and then an actionable error.
- [ ] An antivirus scanner running during extraction does not corrupt the deployment.
- [ ] Killing the manager mid-deployment and restarting recovers the operation.

### SMAPI

- [ ] Fresh SMAPI install from the pinned release succeeds and is verified afterwards.
- [ ] Reinstalling over an existing SMAPI installation behaves as documented.
- [ ] An installer failure is reported and leaves no half-installed state.
- [ ] A wrong hash is refused before the installer runs.
- [ ] An interrupted download resumes or restarts safely.
- [ ] Installation into a path containing spaces and a non-default drive succeeds.
- [ ] The manager restarted after installation still reports SMAPI as installed.

### Launch

- [ ] Vanilla launch starts the game.
- [ ] Modded launch starts the game through SMAPI with `--mods-path`.
- [ ] Runtime test launch behaves as documented.
- [ ] A custom `--mods-path` is honoured and confirmed in the SMAPI log.
- [ ] The correct `%APPDATA%\StardewValley\ErrorLogs\SMAPI-latest.txt` is used for verification.
- [ ] Mod load is confirmed in the SMAPI log for the active profile.
- [ ] Launching while the game is already running is refused with an explanation.
- [ ] Launching while SMAPI is already running is refused with an explanation.
- [ ] Closing the manager leaves the game running.
- [ ] Restarting the manager while the game runs still detects the game.
- [ ] A recycled pid is never mistaken for the game.
- [ ] A process this session started can be terminated from the manager.
- [ ] A process started by Steam rather than by the manager is refused, with instructions to exit from the game menu.

### Installer and application

- [ ] The NSIS installer completes without administrator privileges.
- [ ] The MSI installer completes.
- [ ] The application launches from the Start menu.
- [ ] The embedded frontend loads and IPC works from the installed location.
- [ ] Upgrading from the previous release keeps application data and profiles.
- [ ] Uninstalling removes the application and leaves game directories untouched.
- [ ] The documented application-data policy is what actually happens on uninstall.
- [ ] Authenticode reports `Valid` for the executable and both installers.
- [ ] The application starts correctly on a machine where WebView2 was installed by the bootstrapper.

## Linux acceptance

Run on Fedora x86_64.

- [ ] Fresh clone installs with a frozen pnpm lockfile and locked Cargo dependencies.
- [ ] Formatting, Clippy, Rust tests, TypeScript, frontend tests and the native build pass.
- [ ] Dependency vulnerability and redistribution-license inventory reviewed.
- [ ] SMAPI pinned URL, digest, release and source are verified against the actual downloaded release.
- [ ] The real installer is tested for success, invalid path, permission failure and redirected input and output, with post-install artifacts checked.
- [ ] RPM installed on target Fedora; desktop-menu launch succeeds.
- [ ] Real mod inspected and installed; managed Mods path and bundled-mod behavior verified.
- [ ] Game reaches the title screen; intended mod behavior confirmed in-game.
- [ ] Manager closes while the game survives; reopening avoids duplicate launch and unsafe termination.
- [ ] Removal scope, dependency impact and repeat and restart recovery verified.
- [ ] Keyboard dialog behavior, scaling and multi-monitor positioning checked.
- [ ] Pre-release notes list tested configurations and remaining limitations; SHA-256 checksums published with the bundles.
