# CI and packaging

The repository has one GitHub Actions workflow, `.github/workflows/ci.yml`, with three intentionally different responsibilities:

```text
Quality / Ubuntu ───────────────▶ Package / Fedora RPM smoke
Portability / Linux ────────────┐
Portability / Windows ──────────┼── independent matrix
Portability / macOS ────────────┘
```

The package job depends only on `Quality / Ubuntu`; the portability matrix is an independent compatibility signal and does not delay packaging.

All jobs run for pull requests and pushes to `main`. Superseded runs for the same ref are cancelled. There are no path filters, so required checks cannot be left permanently pending by a narrowly scoped change.

## Quality / Ubuntu

This is the single authoritative fast gate for platform-independent work. It runs frontend formatting, Biome linting, TypeScript checking, Vitest, the production web build, Rust formatting, Clippy with warnings denied, the full locked Rust workspace tests, and generated DTO drift detection.

The job uses the Node and Rust versions pinned in the repository. The Rust workspace test step runs the ts-rs exporter once; `bindings:check` then copies and formats that output deterministically and fails if the canonical checked-in destination changes. This avoids running the `manager-app` tests a second time.

## Portability matrix

Linux, Windows, and macOS each compile the Rust/Tauri workspace. They also run the small pure `manager-core` test suite, which catches portable semantic regressions without repeating the full SQLite/filesystem integration suite and frontend checks three times.

## Fedora RPM smoke

The Fedora job waits for the Ubuntu quality gate. It installs native packaging dependencies, uses Corepack with the repository `packageManager`, builds the RPM, launches the packaged application under Xvfb, and uploads the smoke log and RPM. It does not rerun generic linting, frontend tests, Clippy, or the full Rust suite.

## Local equivalents

Run the complete fast gate locally with:

```sh
pnpm install --frozen-lockfile
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
pnpm bindings:check  # run after the Rust test step, which generates the source bindings
```

The native packaging smoke test is environment-specific; use `pnpm desktop:build` on Fedora with the dependencies listed in the workflow.
