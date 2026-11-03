# Contributing

Keep changes focused on correctness, accessibility and the existing install/launch/remove workflow. Windows and Linux are supported; further platforms, distribution integrations and automatic mod updates are outside the current scope. The application follows the documented modular-monolith architecture; do not add new compatibility APIs or custom routing/query infrastructure.

Follow the platform setup in [README.md](README.md). Use the pinned Bun (`.bun-version`) and Rust (`rust-toolchain.toml`) versions, and commit both lockfiles (`bun.lock` and `Cargo.lock`) when dependencies change. Add npm packages with `bun add`, which writes exact versions and refuses releases younger than three days (`bunfig.toml`); see [dependency updates](docs/dependency-updates.md).

## Platform changes

Add host-specific behaviour to `crates/manager-infra/src/platform/` behind a port, never as a `cfg!` branch inside a service. `HostPlatform::for_host()` is the only selection point. Anything that is data rather than an API call belongs in `platform/shared` so every host can reason about every platform.

Two rules that are easy to get wrong:

- A path is not a string identity. Compare locations with `manager_core::path_semantics`, which models the host's case and separator rules.

- A pid is not a process identity. Termination requires the pid, the creation timestamp and the image path to still match, and a process this session did not start is refused rather than killed.

## Required checks

```sh
bun install --frozen-lockfile
bun scripts/verify-lockfile-age.ts  # bun.lock against the three-day release age; needs the npm registry
bun run format:check
bun run lint
bun run typecheck    # fails if Bun's bundled TypeScript differs from the typescript devDependency
bun run check        # format:check, lint, typecheck, test and build in one go
bun run test
bun test ./scripts   # the helper scripts' own tests
bun run build
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo check --locked --workspace
bun run bindings:check  # run after cargo test; use bun run bindings:generate when regenerating alone
```

At the repository root, plain `bun test` runs every test file without the frontend's jsdom preload and fails; use `bun run test` for the frontend and `bun test ./scripts` for the helpers. `bun scripts/verify-lockfile-age.ts` exits 1 when a locked package is younger than the policy allows and 2 when it could not check at all (for example, offline); exit 2 is never a pass.

The CI layout and job responsibilities are documented in [docs/ci.md](docs/ci.md). Desktop builds are `bun run desktop:build:windows` for the NSIS installer and MSI, `bun run desktop:build:linux` for RPM and DEB, and `bun run desktop:build:appimage` for an AppImage; `bun run desktop:build` uses whatever the host platform's configuration declares. CI builds both platforms and uploads unendorsed artifacts. Only the manual checklists in [docs/release-acceptance.md](docs/release-acceptance.md) qualify a downloadable release, and a platform is only supported once its own checklist has been completed on that platform.

## Frontend tests

The frontend tests run under `bun test` with jsdom and React Testing Library ([ADR-0004](docs/adr/0004-frontend-testing-bun-test-rtl.md)). In `apps/desktop`:

```sh
bun run test                          # the whole suite: bun test --parallel
bun run test src/test/Diagnostics     # files whose path contains the text
bun run test -t "says when the log"   # tests whose name matches
bun run test --coverage               # text summary and coverage/lcov.info
```

Use `bun run test` rather than plain `bun test`, which runs every file in one shared global and fails. Import test APIs from `"bun:test"`. `src/test/setup.ts` provides the jest-dom matchers, Testing Library cleanup, a 5-second wait for `findBy`/`waitFor` and a 30-second per-test timeout. Bun's `vi` has no `vi.mocked` or `vi.hoisted`: write `fn as Mock<typeof fn>` for the first and plain code before `vi.mock` for the second.

## Structure

- `manager-core`: pure models, manifest/dependency rules and deterministic domain algorithms.
- `manager-app`: application services and semantic ports; it must not perform filesystem, process or environment I/O.
- `manager-infra`: SQLite, archives, file locking, SMAPI installer, launcher, log reader, and the per-platform adapters under `platform/`.
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
