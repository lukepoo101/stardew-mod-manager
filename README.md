# Stardew Mod Manager

An **experimental Linux-only MVP** for managing Stardew Valley mods, built with Tauri 2, Rust, React and TypeScript. Fedora x86_64 is the initial packaging target. This is an independent community project, not an official Stardew Valley or SMAPI product.

The app guides you through choosing a fresh native game installation, installing pinned SMAPI 4.1.10, inspecting local mod ZIPs, installing/removing mods and launching the game with an isolated Mods directory. Existing unmanaged modding setups are outside this MVP.

**Status:** source available for development and testing. A successful build or mock test is not proof of compatibility with a real game installation. See [support and limitations](SUPPORT.md) and the [release acceptance checklist](docs/release-acceptance.md) before using a binary release.

## Development on Fedora

Install the native dependencies:

```sh
sudo dnf install git gcc gcc-c++ make pkgconf-pkg-config webkit2gtk4.1-devel openssl-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel curl zenity
```

Install Rust through rustup, Node **24.21.0** and pnpm **11.19.0**. The repository pins Rust in `rust-toolchain.toml`, Node in `.node-version`, and pnpm in `package.json`.

```sh
git clone https://github.com/lukepoo101/stardew-mod-manager.git
cd stardew-mod-manager
pnpm install --frozen-lockfile
pnpm check
pnpm test
cargo test --locked --workspace
pnpm desktop:dev
```

`pnpm dev` opens only the frontend development server, using a mock backend in a normal browser. Use `pnpm desktop:dev` for the actual native application.

## Build

```sh
pnpm desktop:build
```

RPM output is under `target/release/bundle/rpm/`. Install only a package whose release notes list a tested environment. The RPM requires WebKitGTK, GTK, OpenSSL, curl and zenity. No Node runtime is shipped in the application.

## What changes on your computer

- SMAPI setup modifies the selected game directory using the upstream installer.
- Managed user mods are stored in `$XDG_DATA_HOME/stardew-mod-manager/setups/<setup_id>/Mods` (default `~/.local/share/stardew-mod-manager/...`). Launch uses `--mods-path`.
- State and operation journals are in `state.sqlite3` under the application data directory; installer downloads use `$XDG_CACHE_HOME/stardew-mod-manager` (default `~/.cache/stardew-mod-manager`).
- Removal moves the selected package into a recovery directory. A bundle removal affects every component shown in the confirmation dialog. Recovery files are retained; removal is not an uninstall of SMAPI.
- The app reads the local SMAPI log. Verification is conservative and can be unavailable when session, identity or path evidence is ambiguous.

Mods contain code that runs with your user permissions. Archive containment and checksums do not sandbox mods or establish that a mod is trustworthy. Use trusted sources and keep backups of saves.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [third-party notices](THIRD_PARTY_NOTICES.md). Bug reports should include the app commit/version, distribution and installation method. Redact personal paths and other private information from logs.

Licensed under [MIT](LICENSE).
