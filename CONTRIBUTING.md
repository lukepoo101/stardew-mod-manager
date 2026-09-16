# Contributing

Keep changes focused on correctness, accessibility and the existing install/launch/remove workflow. New platforms, distribution integrations and automatic mod updates are outside the current scope. The application is converging on the documented modular-monolith architecture; do not add new compatibility APIs or custom routing/query infrastructure.

Follow the Fedora setup in [README.md](README.md). Use the pinned Node, pnpm and Rust versions and commit both lockfiles when dependencies change.

## Required checks

```sh
pnpm install --frozen-lockfile
pnpm format:check
pnpm lint
pnpm typecheck
pnpm check
pnpm test
pnpm build
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo check --locked --workspace
```

The desktop build is `pnpm desktop:build`; it includes the frontend build hook. CI builds the native app on Fedora and uploads an unendorsed build artifact. Only the manual acceptance checklist qualifies a downloadable release.

## Structure

- `manager-core`: pure models, manifest/dependency rules and deterministic domain algorithms.
- `manager-app`: application services and semantic ports; it must not perform filesystem, process or environment I/O.
- `manager-infra`: SQLite, archives, file locking, SMAPI installer, launcher and log reader.
- `apps/desktop`: React UI, generated IPC DTO client and thin Tauri commands.

Validate IPC identifiers and ownership before constructing paths. Keep file operations journaled and recoverable; a filesystem move and SQLite transaction are not one atomic operation. Tests must use temporary directories and must never alter the developer’s game, Downloads directory or saves.

## Optional real-mod fixtures

Default tests are synthetic and offline. One ignored legacy fixture test can inspect local copies of Farm Type Manager and Stardew Valley Expanded:

```sh
SMM_MOD_FIXTURE_DIR=/path/to/fixtures cargo test --locked -p manager-infra test_real_world_user_downloads_mods -- --ignored
```

The fixture filenames are listed in that test. Obtain them yourself from their authors; do not commit mod archives or game files. Missing fixtures fail the explicitly requested test. This checks archive handling, not in-game loading.

## Pull requests

Explain the user-visible problem, the change and how it was checked. Include regression tests for security/recovery behavior. Small documentation or styling changes do not need tests that merely mirror the implementation. Keep unrelated formatting or feature changes out of follow-up PRs.

Report vulnerabilities privately using [SECURITY.md](SECURITY.md).
