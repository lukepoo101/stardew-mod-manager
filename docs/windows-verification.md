# What is actually verified on Windows

This document is the honest ledger for Windows support: which claims are enforced automatically, on what host, and which are still manual. It exists because "Windows is supported" is a much larger claim than "the Windows job is green", and the gap between the two is where users get hurt.

The manual acceptance checklist lives in `docs/release-acceptance.md`. This document is about what the automated jobs prove, and about the two things they deliberately cannot.

## Automated, on `windows-latest`

Job: `Package / Windows NSIS + MSI`.

| Claim | Where |
| --- | --- |
| The Rust workspace compiles and its tests pass on Windows | `Test the Rust workspace on Windows` |
| The code is Clippy-clean with warnings denied on Windows | `Run Clippy on Windows` |
| The frontend builds and is embedded into the executable | `Build the frontend`, `Build NSIS and MSI installers` |
| Both the NSIS installer and the MSI are produced | `Verify the installers exist` |
| The NSIS installer installs silently and registers itself with Windows | `Install the NSIS package and verify the installed tree` |
| The installed tree contains the application executable | same step |
| The installed application starts directly and is still running 15 seconds later | `Start the installed application directly and require it to stay alive` |
| The WebView2 runtime actually initialised in that process | same step, which asserts the runtime created its isolated user-data directory |
| The installed application completes the full driven journey | `Drive the full WebView2 journey` |
| The uninstaller is present, runs silently and exits zero | `Uninstall the NSIS package` |

### How the driven journey works, and why the obvious approach does not

A Tauri application can be driven by two different providers, and they are not interchangeable.

| Provider | How it works | On a hosted runner |
| --- | --- | --- |
| `embedded` | The application hosts its own W3C WebDriver server (the `webdriver` cargo feature) and drives its own WebView2 through the runtime's native API | works |
| `external` | `tauri-driver` proxies `msedgedriver`, which launches the application and attaches over the WebView2 debugging port | cannot create a session |

The external provider was the original approach and it does not work on a hosted runner. `msedgedriver` reports `DevToolsActivePort file doesn't exist`, deterministically and on every attempt. That was measured rather than assumed:

- the driver version matched the WebView2 **runtime** exactly, so it was not a version mismatch;
- reserved ports were used, so it was not a collision;
- a direct launch of the very same binary on the same runner stayed alive, so the application was not failing to start;
- the same driver and the same application completed the whole journey on a real Windows 11 machine, so it was not an application defect.

The conclusion is that the runner does not let the WebView2 runtime create a debugging port inside a driver-launched process. No change to this repository can alter that.

The embedded provider removes the dependency: the application starts exactly as it does for a user, and the driver is a thread inside it. The CI build therefore uses `pnpm desktop:build:windows:webdriver`, and the journey runs and passes.

## Verified by hand, on real hardware

Both providers were exercised on Windows 11 x64 with the WebView2 runtime present.

Environment: Windows 11 x64, Rust 1.98.1 (`x86_64-pc-windows-msvc`), WebView2 runtime 153.0.4234.32, `tauri-driver` 2.0.6, Python 3.11.9, Microsoft C++ Build Tools 2022.

### The embedded provider, against the installed package

```sh
python scripts/native-smoke.py \
    --binary "%LOCALAPPDATA%\Stardew Mod Manager\stardew-mod-manager.exe" \
    --output native-smoke-results --require-webview2 --driver-provider embedded
```

Result: `passed`, with `driver_provider` reported as `embedded`. The journey asserted, in order, that the packaged application serves the embedded frontend rather than the dev server, that `window.__TAURI_INTERNALS__` exists, that a game can be registered by hand and a default profile created, that onboarding completes, that SMAPI installs and verifies, that a mod ZIP is inspected, previewed, installed and removed, and that a profile can be created.

### The direct start

```sh
python scripts/native-smoke.py \
    --binary "%LOCALAPPDATA%\Stardew Mod Manager\stardew-mod-manager.exe" \
    --output native-smoke-results --direct-start
```

Result: `passed`. This mode also proves the isolation is real rather than nominal: it fails when the WebView2 runtime does not create its isolated user-data directory, and it does fail when the isolation path is relative, which is exactly the silent fallback to the shared profile the absolute-path requirement exists to prevent.

### The external provider, and the limit it exposes

The external provider drives the application successfully on a real machine and reaches the SMAPI step. It cannot pass that step, for a reason that is worth writing down: `msedgedriver` launches the application with no console, and the upstream SMAPI installer requires one. Without a console it fails, and its own error path calls `Console.ReadKey()`, which throws when there is no console, so the real error is replaced by that exception.

This is a property of how the application was launched, not of the application. Under the embedded provider the application is launched with its own console - the same thing the Windows console adapter already does for the installer - and the step passes. The smoke test therefore reports `blocked`, with that explanation, when it detects the SMAPI installer failing for this reason under an external driver, instead of reporting an application failure.

## What is still manual

Not covered by anything above, and still on the checklist in `docs/release-acceptance.md`: a real Stardew Valley installation, launching the game, real mods loading in game, Steam discovery of a real library, non-ASCII and junction paths, upgrade and uninstall data policy, and Authenticode verification with a real certificate.

## Reproducing it locally

```sh
pnpm desktop:build:windows:webdriver

# The claim that needs no driver:
python scripts/native-smoke.py --binary "%LOCALAPPDATA%\Stardew Mod Manager\stardew-mod-manager.exe" \
    --output native-smoke-results --direct-start

# The full journey, through the application's own WebDriver server:
python scripts/native-smoke.py --binary "%LOCALAPPDATA%\Stardew Mod Manager\stardew-mod-manager.exe" \
    --output native-smoke-results --require-webview2 --driver-provider embedded

# The same journey the old way, needing msedgedriver and tauri-driver on PATH:
python scripts/native-smoke.py --binary "%LOCALAPPDATA%\Stardew Mod Manager\stardew-mod-manager.exe" \
    --output native-smoke-results --require-webview2 --driver-provider external
```

Every mode gives each launch its own WebView2 user-data directory and resolves the configured path to an absolute one first. The WebView2 runtime ignores a relative `WEBVIEW2_USER_DATA_FOLDER` and writes to the shared default profile without reporting anything, so a relative path makes the isolation a fiction. `APPDATA` and `LOCALAPPDATA` are redirected into the same scratch directory so a probe cannot disturb a real user's application data; the application's own data directory is still resolved by Windows to the real one, which is why the smoke run needs a machine where the application has not been set up yet, or a cleared `%APPDATA%\stardew-mod-manager`.

## Why the journey build is not the release build

The `webdriver` cargo feature embeds an HTTP server in the application that answers WebDriver commands. It is off by default and is not enabled by `pnpm desktop:build:windows`, so every artifact a user receives is built without it. The Windows CI job enables it because that job's purpose is to prove the application works at runtime, and the release job builds a separate test-only binary for the same reason. The trade is deliberate: a released binary that can be driven by anything on the machine is a worse outcome than a release job that builds one extra binary.
