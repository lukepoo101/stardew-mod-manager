# CI and packaging

The repository has two GitHub Actions workflows: `.github/workflows/ci.yml` for every pull request and push to `main`, and `.github/workflows/release.yml` for tagged releases.

## CI levels

CI is organised in three levels, because compiling on a platform and running on it are different claims.

```text
Level 1  portable correctness        Quality / Ubuntu, Portability / Linux, Portability / macOS
Level 2  native build and installer  Package / Windows NSIS + MSI, Package / Linux bundles + smoke
Level 3  native functional smoke     WebView2 (Windows) and WebKitGTK (Linux) user journeys
```

```text
Quality / Ubuntu ──┬──▶ Package / Windows NSIS + MSI
                   └──▶ Package / Linux bundles + smoke
Portability / Linux ─┐
Portability / macOS ─┴── independent compatibility signal
```

The package jobs depend only on `Quality / Ubuntu`; the portability matrix is an independent signal and does not delay packaging. All jobs run for pull requests and pushes to `main`; superseded runs for the same ref are cancelled. There are no path filters, so required checks cannot be left permanently pending by a narrowly scoped change.

## Level 1: Quality / Ubuntu

The authoritative fast gate for platform-independent work: frontend formatting, Biome linting, TypeScript checking, Vitest, the production web build, Rust formatting, Clippy with warnings denied, the full locked Rust workspace test suite, and generated DTO drift detection.

The job uses the Node and Rust versions pinned in the repository. The Rust workspace test step runs the ts-rs exporter once; `bindings:check` then copies and formats that output deterministically and fails if the canonical checked-in destination changes.

## Level 1: Portability matrix

Linux and macOS each compile the Rust/Tauri workspace and run the full Rust workspace test suite. Tauri validates that its configured `frontendDist` exists while expanding `generate_context!`, so the matrix creates that directory as a compile-only placeholder; it does not install or rebuild JavaScript dependencies.

Windows is deliberately **not** part of this matrix. It runs the same Rust workspace tests in `Package / Windows NSIS + MSI`, where the application is also built, installed and started. A Windows entry here would duplicate work without adding a claim.

## Level 2 and 3: Package / Windows NSIS + MSI

On `windows-latest`: `cargo test --workspace --locked` and Clippy on the host the build ships to, a frontend build, `pnpm desktop:build:windows` producing both the NSIS installer and the MSI, verification that both were produced, a silent install followed by a real start of the installed application, a silent uninstall, and the WebView2 smoke test.

The WebView2 smoke test is a required step. It installs the Edge Driver that matches the **WebView2 runtime** (which updates independently of the Edge browser) and `tauri-driver`, because a WebView2 application is not driven by `msedgedriver` directly: `tauri-driver` translates the `tauri:options` capability into the edge options the native driver understands.

The step then distinguishes two claims, which is why it records a status rather than only an exit code:

- `passed` - the packaged application completed the full journey. This is the claim that matters.
- `blocked` - the host let the application start but the WebView2 runtime never initialised inside the WebDriver-launched process, so no session could be created. GitHub's hosted runner behaves this way; the step prints a warning naming the limitation and the run uploads its diagnostics.
- anything else - the application failed, and the job fails.

This is deliberate. Reporting an environment that cannot host a WebView2 window as a pass would be a false green, and reporting it as an application failure would be a false alarm. The `blocked` status and its reason are visible in the run log and in the uploaded `native-failure-diagnostics.txt`.

The journey is verified on a real Windows machine, where it passes end to end. The release workflow runs the same step.

The Windows build toolchain needs the Microsoft C++ Build Tools, which the runner image provides, and the VBSCRIPT optional Windows feature for the MSI, which the runner image also provides.

## Level 2 and 3: Package / Linux bundles + smoke

Runs in a Fedora container, installs native packaging dependencies, builds the RPM and DEB packages, launches the packaged application under Xvfb through WebKitWebDriver, and uploads the smoke log and bundles. The user journey driven by the smoke test is shared with Windows.

AppImage is deliberately not in the default Linux bundle set: it requires `linuxdeploy` and FUSE, which the packaging container does not provide. `pnpm desktop:build:appimage` builds it on a host that has them.

## Release

`.github/workflows/release.yml` runs on a `v*` tag or a manual dispatch. It resolves the version, builds and tests on Linux and Windows, produces a draft GitHub release with Linux bundles, the NSIS installer and the MSI, and publishes SHA-256 checksums.

Windows artifacts are Authenticode signed when the `release` environment provides `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD`. Signing covers the application executable, the NSIS installer and the MSI, uses SHA-256 with a trusted timestamp, and the pipeline fails if any artifact does not report a `Valid` signature. Without those secrets the release is built and published unsigned, and the run reports `SIGNED=false`. Signing credentials are never available to a pull request build.

## Local equivalents

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

The native smoke test is environment-specific:

```sh
# Linux, in a graphical session or under xvfb-run
python3 scripts/native-smoke.py --output native-smoke-results

# Windows, with Microsoft Edge Driver on PATH
python scripts/native-smoke.py --output native-smoke-results
```

On Windows the script always verifies the installed application tree and reports `skipped` when Edge Driver is unavailable, unless `--require-webview2` is passed.
