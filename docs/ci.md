# CI and packaging

The repository has three GitHub Actions workflows: `.github/workflows/ci.yml` for every pull request and push to `main`, `.github/workflows/dependency-health.yml` for dependency audits and Renovate validation, and `.github/workflows/release.yml` for tagged releases. CI and dependency health also run for `merge_group`; a merge queue is not currently required.

## CI levels

CI is organised in three levels, because compiling on a platform and running on it are different claims.

```text
Level 1  portable correctness        Quality / Ubuntu, Test / Windows, Portability / macOS
Level 2  native build and installer  Package / Windows NSIS + MSI, Package / Linux bundles + smoke
Level 3  native functional smoke     WebView2 (Windows) and WebKitGTK (Linux) user journeys
```

```text
Quality / Ubuntu ────────────────┐
Test / Windows ──────────────────┤
Portability / macOS ─────────────┼── all start together; none waits for another
Package / Windows NSIS + MSI ────┤
Package / Linux bundles + smoke ─┘
```

No job depends on another. The package jobs use nothing that `Quality / Ubuntu` produces, and waiting for it put its whole duration in front of the longest job. All jobs run for pull requests and pushes to `main`. A newer push to a pull request cancels the run before it; runs on `main` are left to finish, because they are the ones that save the build caches. There are no path filters, so required checks cannot be left permanently pending by a narrowly scoped change.

## Level 1: Quality / Ubuntu

The authoritative fast gate for platform-independent work: the lockfile release-age check, frontend formatting, Biome linting, TypeScript checking (`bun check`, after a guard that Bun's bundled TypeScript matches the `typescript` devDependency), the frontend tests (`bun test` with jsdom), the helper scripts' tests (`bun test ./scripts`), the production web build, Rust formatting, Clippy with warnings denied, the full locked Rust workspace test suite, and generated DTO drift detection.

The job installs Tauri's system libraries with `scripts/install-linux-ci-deps.sh`, which bounds each apt attempt and retries it, so a stalled package mirror fails the attempt within minutes instead of holding the job until the runner's time limit. A healthy install takes about 25 seconds and an attempt is cut off after a few minutes. After a failed attempt the script drops the runner's default mirror (`azure.archive.ubuntu.com`, the one that stalls) from the runner's mirror list, so the retry uses `archive.ubuntu.com`.

### Build caches

Each job that compiles Rust restores the cache its own job saved, through `Swatinem/rust-cache`, and only runs on `main` save one. A cache saved by a pull request can be read by that pull request alone, and at one to two gigabytes each they pushed the repository past GitHub's 10 GB cache limit, which evicted the caches from `main` that every other run restores. A pull request that changes `Cargo.lock` therefore recompiles the changed dependencies on each of its runs.

The job uses the Bun and Rust versions pinned in the repository. The Rust workspace test step runs the ts-rs exporter once; `bindings:check` then copies and formats that output deterministically and fails if the canonical checked-in destination changes.

Bun is read from `.bun-version` by `oven-sh/setup-bun`, and Rust from `rust-toolchain.toml`. setup-bun falls back to the latest Bun, with only a warning, when it cannot read the file, so every job that installs packages first checks that `bun --version` equals the pin. The setup steps read Rust's channel through `scripts/read-rust-toolchain.ts`; Fedora activates that same manifest through rustup. No job sets up Node.js or Python: Vite, Biome and the Tauri CLI run on Bun's runtime (`--bun`), so none of the repository's commands runs on the Node.js that GitHub's runner images include. Actions use commit pins and Fedora uses a digest pin. All `desktop:build*` commands forward `--locked` to Cargo.

Every job that installs JavaScript packages runs `bun install --frozen-lockfile` and then `bun scripts/verify-lockfile-age.ts`. A frozen install does not re-check `bun.lock` against the three-day `minimumReleaseAge` in `bunfig.toml`; the verifier dates every locked package against the npm registry and fails (exit 1) on one that is too young. Exit 2 means the check could not run, usually because the registry could not be reached. The step fails with an annotation that says so, and the job should be re-run: an unverified lockfile is never a pass. Quality, both Package jobs, and both release build jobs run it. Quality and both Package jobs cache Bun's package cache keyed on `bun.lock`; the release jobs download every package fresh, as they did before.

## Dependency health and required checks

`Dependencies / Audit + Renovate` runs strict Renovate configuration validation (through `bunx`, resolving only releases at least three days old), `bun audit`, and `cargo audit --deny unsound`. It runs on every PR, main push, merge group, daily at 05:00 UTC, and manual dispatch. It uploads a complete Rust advisory report, including the single upstream `glib` exception documented in [dependency updates](dependency-updates.md).

The `main` ruleset requires a PR, an up-to-date branch, and these checks from the GitHub Actions app: `Quality / Ubuntu`, `Test / Windows`, `Portability / macos-latest`, both `Package` jobs, `Dependencies / Audit + Renovate`, and CodeQL's `Analyze (actions)`, `Analyze (javascript-typescript)`, and `Analyze (rust)` jobs. A code-scanning rule also requires CodeQL results and blocks security alerts at any severity and ordinary error-level alerts. There is no Renovate bypass or human-approval requirement; Renovate only requests automerge for eligible updates.

CodeQL runs as GitHub's default setup, configured in the repository's code security settings rather than in a workflow file. Its languages are Actions, JavaScript/TypeScript and Rust. Python is not analysed: the repository has had no Python since the helper scripts and the native smoke test moved to Bun, so a Python analysis would have nothing to scan.

## Level 1: Portability / macOS and Test / Windows

`Portability / macos-latest` compiles the Rust/Tauri workspace and runs the full Rust workspace test suite. Linux has no job of its own here: `Quality / Ubuntu` already compiles and tests the same workspace on that host.

`Test / Windows` runs `cargo test --workspace --locked` and Clippy on the host the build ships to. The Rust tests run with the temporary directory on the runner's work drive: the fixtures are file-heavy, and on the default `C:` location one test binary that takes 5 seconds on Linux took over 3 minutes.

Tauri validates that its configured `frontendDist` exists while expanding `generate_context!`, so both jobs create that directory with a placeholder `index.html`; neither installs JavaScript dependencies or builds the frontend.

The Windows tests are a separate job from the Windows package so that the two run side by side. One is a debug build and the other a release build, so they share no compiled output, and one after the other they were the longest path through the workflow.

## Level 2 and 3: Package / Windows NSIS + MSI

On `windows-latest`: the helper scripts' tests on Windows (here rather than in `Test / Windows`, because they need the installed packages), a frontend build, `bun run desktop:build:windows:webdriver` producing both the NSIS installer and the MSI, verification that both were produced, a silent install, a check of the installed tree, a run of the installed application that must stay alive, the full driven journey, and a silent uninstall.

### The two claims Windows makes

They are separated because they fail for different reasons, and collapsing them is how a smoke test becomes either a false alarm or a false green.

**1. The installed application starts and stays running (no driver involved).**

`Start the installed application directly and require it to stay alive` launches the installed executable the way a user does and requires it to still be running after 15 seconds. It uses no WebDriver, so nothing about the host's automation support can defeat it, and an application that exits during start-up fails here. The job fails if it fails.

The launch is given its own WebView2 user-data directory under `native-smoke-direct-start/isolation`, and its own `APPDATA`/`LOCALAPPDATA`. That isolation is not cosmetic. The WebView2 runtime takes an exclusive lock inside its user-data directory and silently falls back to the shared default profile when the configured path is relative, so each launch is given a distinct absolute path and the run asserts that the runtime created it: an application whose window never started cannot produce that directory.

**2. The installed application completes the full driven journey.**

`Drive the full WebView2 journey` drives the installed application through onboarding, manual game registration, SMAPI installation, mod ZIP inspection, installation and removal, and profile creation. It is a required pass.

What makes that possible on a hosted runner is which process hosts the WebDriver. Two providers can drive a Tauri application:

| Provider | How it works | Works on a hosted runner |
| --- | --- | --- |
| `embedded` | The application hosts its own W3C WebDriver server (`tauri-plugin-wdio-webdriver`, behind the `webdriver` cargo feature) and drives its own WebView2 through the runtime's native API | Yes |
| `external` | `tauri-driver` proxies `msedgedriver`, which launches the application and attaches over the WebView2 debugging port | No |

The external provider is the one a hosted runner defeats: the runtime never creates a debugging port inside a driver-launched process, so no session can be created at all. That is a property of the host, not of the application, and no change to this repository can fix it.

The embedded provider has no such dependency. The application starts exactly as it does for a user, and the driver is a thread inside it, so the journey runs where the external provider cannot. The CI build enables the feature for this reason; the second, separate isolation directory under `native-smoke-results/isolation` keeps the two launches from sharing a WebView2 profile.

A build that lost the feature fails loudly rather than quietly: the journey names `--driver-provider embedded` instead of auto-detecting, so a missing provider is reported as a missing provider. The run still records three outcomes, because the same script is used by hand and on other hosts:

- `passed` - the application completed the full journey.
- `blocked` - no WebDriver session could be created at all, and the reason is the host rather than the product. CI treats this as a failed step, because it built the application specifically so it would not happen.
- `failed` - the journey started and then failed. The job fails.

`blocked` is narrow on purpose, so it can never hide a broken application. Only three conditions produce it: the host refusing to let an external driver attach, an application without an embedded WebDriver server, and an application launched by an external driver being asked to run the upstream SMAPI installer, which requires a console that such a driver does not give it. Everything else, including a journey that starts and then fails, fails the job. The status and its reason are visible in the run log and in the uploaded `native-failure-diagnostics.txt`.

The release workflow makes both claims. Because a released artifact must not contain a process that answers WebDriver commands, that job builds a second, test-only binary with the feature enabled and drives the journey through it.

See `docs/windows-verification.md` for what was verified where, and with which commands.
## Level 2 and 3: Package / Linux bundles + smoke

Runs in a Fedora container, installs native packaging dependencies, builds the RPM and DEB packages, launches the packaged application under Xvfb through WebKitWebDriver, and uploads the smoke log and bundles. The user journey driven by the smoke test is shared with Windows.

AppImage is deliberately not in the default Linux bundle set: it requires `linuxdeploy` and FUSE, which the packaging container does not provide. `bun run desktop:build:appimage` builds it on a host that has them.

## Release

`.github/workflows/release.yml` runs on a `v*` tag or a manual dispatch. It resolves the version, builds and tests on Linux and Windows, produces a draft GitHub release with Linux bundles, the NSIS installer and the MSI, and publishes SHA-256 checksums.

Windows artifacts are Authenticode signed when the `release` environment provides `WINDOWS_CERTIFICATE_PFX` (a base64 PKCS#12 export) and `WINDOWS_CERTIFICATE_PASSWORD`. A thumbprint alone is not a credential and cannot be used on an ephemeral runner, so the pipeline materialises the certificate, imports it into the runner's current-user store, and reads the resulting thumbprint back.

Signing covers the application executable, the NSIS installer and the MSI, uses SHA-256 with a trusted timestamp, and the pipeline fails if any artifact does not report a `Valid` signature **from that certificate**. Without the secrets the release job fails before it builds rather than publishing unsigned artifacts by accident. Signing credentials are never available to a pull request build.


## Local equivalents

```sh
bun install --frozen-lockfile
bun scripts/verify-lockfile-age.ts  # exit 2 means it could not check; re-run, never a pass
bun run format:check
bun run lint
bun run typecheck
bun run test
bun test ./scripts
bun run build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
bun run bindings:check  # run after the Rust test step, which generates the source bindings
```

The native smoke test is environment-specific:

```sh
# Linux, in a graphical session or under xvfb-run
bun scripts/native-smoke.ts --output native-smoke-results

# Windows: launch the installed application and require it to stay alive.
# Needs no driver, and always holds.
bun scripts/native-smoke.ts --output native-smoke-results --direct-start

# Windows: drive the full journey through the application's own WebDriver
# server. Needs a build made with `bun run desktop:build:windows:webdriver`.
bun scripts/native-smoke.ts --output native-smoke-results --require-webview2 --driver-provider embedded
```

On Windows the script always verifies that the executable is a real PE image, and reports `skipped` when no provider is available unless `--require-webview2` is passed. `--driver-provider` selects `embedded`, `external` or `auto`, where `auto` prefers the embedded provider and falls back to the external one. `--direct-start` and a driven run may be combined; the direct start runs first, so a host that cannot drive a WebView2 window still proves that the application starts.

Both Windows modes isolate every launch in its own WebView2 user-data directory, held under `<output>/webview2-isolation` or the directory passed to `--webview2-isolation`. The path is resolved to an absolute one before use, because the runtime ignores a relative one and quietly writes to the shared default profile instead.
