# Support and known limitations

This is an experimental Linux-only MVP. Fedora x86_64 is the first build/packaging target. Development checks have run on Fedora 44; real-game and installed-RPM acceptance remains a separate requirement. No Windows/macOS support is claimed. Other Linux distributions, Flatpak Steam, GOG and alternative architectures are unverified.

The intended game is a fresh native Linux Stardew Valley installation compatible with pinned SMAPI 4.1.10. The configured upstream minimum is 1.6.9; this is not an acceptance-tested compatibility range. Existing unmanaged SMAPI/mod setups cannot be migrated by this MVP. Validate real game-version metadata before claiming a supported build.

## Current limits

- A browser preview uses a mock backend and does not install or launch anything.
- Session verification may be unavailable, including anonymized paths or ambiguous/missing mod identity evidence. It is never proof that every mod behaves correctly in-game.
- After restarting the manager, stopping an untracked game PID is refused. Exit through the game menu instead.
- Interrupted SMAPI setup may need manual reconciliation. The journal is retained and conflicting operations are blocked; do not delete state to bypass a recovery error.
- Quarantined removals retain disk space. There is no recovery-bin UI yet.
- SMAPI’s bundled mods remain in its game directory; the managed setup uses a separate Mods path. Bundled-mod behavior must be included in real acceptance testing.
- Automatic updates, mod downloads from marketplaces, profiles and additional platforms are outside this version.

Use “Change Game Folder” to select a different native installation. Use the mod ZIP picker or an exact path. Do not recursively change game permissions as a generic troubleshooting step.

For bugs, include distribution/version, app commit, installation method, reproduction steps and redacted logs. Do not attach saves, game binaries, tokens or private data. See [release acceptance](docs/release-acceptance.md).
