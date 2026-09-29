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

On `windows-latest`: `cargo test --workspace --locked` and Clippy on the host the build ships to, a frontend build, `pnpm desktop:build:windows:webdriver` producing both the NSIS installer and the MSI, verification that both were produced, a silent install, a check of the installed tree, a run of the installed application that must stay alive, the full driven journey, and a silent uninstall.

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

AppImage is deliberately not in the default Linux bundle set: it requires `linuxdeploy` and FUSE, which the packaging container does not provide. `pnpm desktop:build:appimage` builds it on a host that has them.

## Release

`.github/workflows/release.yml` runs on a `v*` tag or a manual dispatch. It resolves the version, builds and tests on Linux and Windows, produces a draft GitHub release with Linux bundles, the NSIS installer and the MSI, and publishes SHA-256 checksums.

Windows artifacts are Authenticode signed when the `release` environment provides `WINDOWS_CERTIFICATE_PFX` (a base64 PKCS#12 export) and `WINDOWS_CERTIFICATE_PASSWORD`. A thumbprint alone is not a credential and cannot be used on an ephemeral runner, so the pipeline materialises the certificate, imports it into the runner's current-user store, and reads the resulting thumbprint back.

Signing covers the application executable, the NSIS installer and the MSI, uses SHA-256 with a trusted timestamp, and the pipeline fails if any artifact does not report a `Valid` signature **from that certificate**. Without the secrets the release job fails before it builds rather than publishing unsigned artifacts by accident. Signing credentials are never available to a pull request build.


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

# Windows: launch the installed application and require it to stay alive.
# Needs no driver, and always holds.
python scripts/native-smoke.py --output native-smoke-results --direct-start

# Windows: drive the full journey through the application's own WebDriver
# server. Needs a build made with `pnpm desktop:build:windows:webdriver`.
python scripts/native-smoke.py --output native-smoke-results --require-webview2 --driver-provider embedded
```

On Windows the script always verifies that the executable is a real PE image, and reports `skipped` when no provider is available unless `--require-webview2` is passed. `--driver-provider` selects `embedded`, `external` or `auto`, where `auto` prefers the embedded provider and falls back to the external one. `--direct-start` and a driven run may be combined; the direct start runs first, so a host that cannot drive a WebView2 window still proves that the application starts.

Both Windows modes isolate every launch in its own WebView2 user-data directory, held under `<output>/webview2-isolation` or the directory passed to `--webview2-isolation`. The path is resolved to an absolute one before use, because the runtime ignores a relative one and quietly writes to the shared default profile instead.
