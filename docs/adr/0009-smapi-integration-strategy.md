# ADR-0009: SMAPI Integration Strategy - Unattended CLI Invocation with Post-Install Artifact Validation

## Status
Accepted

## Date
2026-09-12

## Context
SMAPI is distributed upstream as an interactive installer package containing `SMAPI.Installer` (or `internal/linux/SMAPI.Installer`). Upstream source code inspection reveals CLI options:
`--install`, `--uninstall`, `--game-path "<path>"`, and `--no-prompt`.
However, upstream tests and real-world behavior reveal subtle pitfalls:
1. When passed an invalid game directory, `SMAPI.Installer` prints an error message but exits with code 0. Therefore, exit code 0 does NOT guarantee successful installation.
2. The installer can block indefinitely if interactive prompts are triggered unless standard input is disconnected or redirected (`/dev/null`).
3. Running `StardewModdingAPI --version` directly can fail if invoked outside the game's .NET host runtime environment.

## Decision
1. **Pinned Release**:
   - Pin SMAPI to release **4.1.10** (SHA-256: `8c127148a76c890e485aea73910189dc41c80f822c2a0a5e3b0e762b4ee3a93e`, source tag: `4.1.10`, commit: `fd73446090cd71f4948f34ba8c428e45aa0a3ebf`).
2. **Safe Invocation**:
   - Invoke `./internal/linux/SMAPI.Installer --install --game-path "<game_path>" --no-prompt` with `stdin` bound to `/dev/null` and capture all stdout/stderr streams.
3. **Artifact-Level Verification**:
   - Do not rely on process exit code alone.
   - Verify stdout contains confirmation text (`"SMAPI is installed!"`).
   - Validate post-install filesystem artifacts: verify existence of `StardewModdingAPI`, `StardewModdingAPI.dll`, `StardewModdingAPI.deps.json`, `smapi-internal/`, and bundled mods (`ConsoleCommands/`, `SaveBackup/`).

## Consequences
### Positive
- Fully automated, silent, unattended installation without user terminal interaction.
- Artifact-based detection of failed installs, corrupt paths, or permission issues.
- Protection against upstream installer exit code anomalies.

### Negative
- Pinning SMAPI requires explicit version updates when new SMAPI releases are published.

## Alternatives Considered
- **Direct manual file copying of SMAPI files into game directory**: Upstream installer performs OS-specific assembly patching, permissions configuration, and wrapper setup. Using the official installer ensures compatibility with upstream expectations.
