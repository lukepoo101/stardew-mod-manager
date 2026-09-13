# ADR-0007: Mod Isolation and Deployment Strategy - Profile-Specific Isolated Mods Directories via `--mods-path`

## Status
Updated (originally Accepted 2026-09-12; updated 2026-09-13 to profile-specific isolation)

## Date
2026-09-12 (Updated 2026-09-13)

## Context
Traditional mod managers often dump files directly into the game's default `Mods/` directory inside the Steam game installation path. This causes several critical problems:
1. It pollutes the game directory and breaks game integrity checks in Steam.
2. Unmanaged mods or legacy leftovers can conflict with managed mods.
3. Multiple profiles or clean testing states cannot be easily achieved.
4. SMAPI officially supports the `--mods-path <path>` command-line argument to specify an alternate directory from which mods should be loaded.

## Decision
Deploy managed mods to an application-managed, profile-specific directory (physically located at `setups/<profile-id>/Mods` under the application data directory) and launch SMAPI with `--mods-path "<path>"`. Bundled SMAPI mods (`ConsoleCommands`, `SaveBackup`) reside in the game directory or can be preserved, while user mods are strictly isolated per profile.

The physical storage path is managed by infrastructure through `AppPaths` and `ManagedPaths`, deriving the isolated directory deterministically from the trusted `ProfileId`.

## Consequences
### Positive
- Leaves the vanilla game directory completely clean and unmodified.
- Steam file integrity checks never overwrite or conflict with user mods.
- True multi-profile support: each profile has its own completely isolated `Mods/` directory.
- Easy uninstallation and cleanup: deleting a profile directory fully removes all mod state without residual files in Steam.
- Mod load verification can inspect SMAPI's log specifically for the custom `--mods-path` banner: `"Mods go here: <path>"`.

### Negative
- Launching the game directly via the Steam client shortcut will load from default `Mods/` rather than the manager's directory unless the user configures launch options (`StardewModdingAPI %command% --mods-path ...`).
- Mod authors assuming hardcoded relative paths to game root from `Mods/` could encounter path differences (rare in modern SMAPI mods).

## Alternatives Considered
- **Direct in-tree `Mods/` writing**: Discarded due to pollution, collision risks, and inability to maintain clean separation.
- **Single global isolated `Mods/` folder**: Discarded because switching profiles would require physically swapping or re-linking files.
- **Symlinking / Hardlinking into `Mods/`**: Adds filesystem complexity, symlink permission issues on some filesystems, and residual broken symlinks on deletion.
