# Support and known limitations

This is an experimental mod manager for **Windows and Linux**. Windows 10 22H2 x64 and Windows 11 x64 are the intended Windows targets; Fedora x86_64 is the intended Linux target. Platform acceptance is tracked in [release acceptance](docs/release-acceptance.md) and is not implied by a green test suite. macOS, GOG automatic discovery, Flatpak Steam and alternative architectures are unverified.

The intended game is a fresh native Stardew Valley installation of the host platform compatible with pinned SMAPI 4.1.10. The configured upstream minimum is 1.6.9; this is not an acceptance-tested compatibility range. Existing unmanaged SMAPI or mod setups cannot be migrated by this version.

## Platform notes

- **Windows.** The manager starts `StardewModdingAPI.exe` directly rather than through Steam, so the Steam overlay, playtime tracking and achievements are not driven by Steam for a modded launch. Start the game from Steam if you want those.
- **Windows.** Process ownership is proved with the pid, its kernel creation timestamp and its image path. Closing the manager never terminates Stardew Valley.
- **Linux.** Proton and Wine prefixes present a Windows file set. They are recognised and reported as a Windows installation on a Linux host, and cannot be managed by this version.
- **Both.** A game installation is identified by comparing locations with the host's path rules, so on a case-insensitive filesystem `C:\Games\Stardew Valley` and `c:/games/stardew valley` are one installation.
- **Both.** Archives are validated against the Windows file-namespace rules on every platform, so a package that could not be extracted on Windows is rejected before extraction on Linux too.

## Current limits

- A browser preview uses a mock backend and does not install or launch anything.
- Session verification may be unavailable, including anonymized paths or ambiguous or missing mod identity evidence. It is never proof that every mod behaves correctly in-game.
- After restarting the manager, stopping a game process this session did not start is refused. Exit through the game menu instead.
- Interrupted SMAPI setup may need manual reconciliation. The journal is retained and conflicting operations are blocked; do not delete state to bypass a recovery error.
- Quarantined removals retain disk space. There is no recovery-bin UI yet.
- SMAPI's bundled mods remain in its game directory; the managed setup uses a separate Mods path. Bundled-mod behavior must be included in real acceptance testing.
- Automatic updates, mod downloads from marketplaces and additional platforms are outside this version.

Use "Change Game Folder" to select a different native installation. Use the mod ZIP picker or an exact path. Do not recursively change game permissions as a generic troubleshooting step.

For bugs, include the platform and version, app commit, installation method, reproduction steps and redacted logs. The Diagnostics screen reports the host platform, the application data and cache directories, the Steam locations that were searched, and where SMAPI writes its log on each supported platform. Do not attach saves, game binaries, tokens or private data. See [release acceptance](docs/release-acceptance.md).
