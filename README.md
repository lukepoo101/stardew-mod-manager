# Stardew Mod Manager

An **experimental mod manager for Stardew Valley on Windows and Linux**, built with Tauri 2, Rust, React and TypeScript. This is an independent community project, not an official Stardew Valley or SMAPI product.

The app guides you through choosing a fresh native game installation, installing pinned SMAPI 4.1.10, inspecting local mod ZIPs, installing and removing mods, and launching the game with an isolated Mods directory. Existing unmanaged modding setups are outside this version.

**Status:** source available for development and testing. A successful build or mock test is not proof of compatibility with a real game installation. See [support and limitations](SUPPORT.md) and the [release acceptance checklists](docs/release-acceptance.md) before using a binary release.

## Supported platforms

| Platform              | Discovery             | Packaging          | Automated on every change                | Acceptance status               |
| --------------------- | --------------------- | ------------------ | ---------------------------------------- | ------------------------------- |
| Windows 10 22H2 x64   | Steam + manual folder | NSIS .exe, MSI     | Build, install, start, driven journey    | Required before a Windows claim |
| Windows 11 x64        | Steam + manual folder | NSIS .exe, MSI     | Build, install, start, driven journey    | Required before a Windows claim |
| Fedora x86_64 (Linux) | Steam + manual folder | RPM, DEB, AppImage | Build, install, driven journey           | Required before a Linux claim   |

macOS is not supported. The platform abstraction is designed so it can be added as another adapter, but no macOS adapter exists yet.

What the automated column does and does not cover is written down in [what is actually verified on Windows](docs/windows-verification.md). A green job is not acceptance, and the manual checklist in [release acceptance](docs/release-acceptance.md) still applies.

Modded launches start the game through SMAPI directly, which is what makes per-profile mod isolation work. Launching through Steam instead would add the Steam overlay and playtime tracking but cannot take a profile-specific mods path, so storefront-integrated launch is a deliberate non-goal for this release.

## Development

### Windows

Install the Microsoft C++ Build Tools with the "Desktop development with C++" workload, WebView2 (already present on Windows 10 22H2 and Windows 11), Rust through rustup, Node **24.21.0** and pnpm. The repository pins Rust in `rust-toolchain.toml`, Node in `.node-version`, and pnpm in `package.json`.

```powershell
git clone https://github.com/lukepoo101/stardew-mod-manager.git
cd stardew-mod-manager
pnpm install --frozen-lockfile
pnpm check
pnpm test
cargo test --locked --workspace
pnpm desktop:dev
```

### Fedora / Linux

```sh
sudo dnf install git gcc gcc-c++ make pkgconf-pkg-config webkit2gtk4.1-devel openssl-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel curl
```

The remaining commands are the same as above.

`pnpm dev` opens only the frontend development server, using a mock backend in a normal browser. Use `pnpm desktop:dev` for the actual native application.

## Build

```sh
pnpm desktop:build:windows   # NSIS .exe and MSI, under target/release/bundle/{nsis,msi}
pnpm desktop:build:linux     # RPM, DEB and AppImage, under target/release/bundle
pnpm desktop:build           # whatever the host platform's configuration declares
```

Install only a package whose release notes list a tested environment. No Node runtime is shipped in the application.

## What changes on your computer

- SMAPI setup modifies the selected game directory using the upstream installer.
- Managed user mods are stored in the application data directory:
  - Windows: `%APPDATA%\stardew-mod-manager\setups\<profile_id>\Mods`
  - Linux: `$XDG_DATA_HOME/stardew-mod-manager/setups/<profile_id>/Mods` (default `~/.local/share/...`)
  Launch passes that directory to SMAPI through `--mods-path`.
- State and operation journals live in `state.sqlite3` in the application data directory; installer downloads use the application cache directory:
  - Windows: `%LOCALAPPDATA%\stardew-mod-manager`
  - Linux: `$XDG_CACHE_HOME/stardew-mod-manager`
- Removal moves the selected package into a recovery directory. A bundle removal affects every component shown in the confirmation dialog. Recovery files are retained; removal is not an uninstall of SMAPI.
- The app reads the local SMAPI log. Verification is conservative and can be unavailable when session, identity or path evidence is ambiguous.

Mods contain code that runs with your user permissions. Archive containment and checksums do not sandbox mods or establish that a mod is trustworthy. Use trusted sources and keep backups of saves.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [third-party notices](THIRD_PARTY_NOTICES.md). Bug reports should include the app commit/version, platform, installation method and distribution. Redact personal paths and other private information from logs.

Licensed under [MIT](LICENSE).
