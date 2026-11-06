# ADR-0018: JavaScript Tooling - Bun

## Status
Accepted. Supersedes [ADR-0003](0003-javascript-tooling-node-pnpm.md).

## Date
2026-11-03

## Context
ADR-0003 chose Node.js 24 and pnpm, and rejected Bun over compatibility with native Node tooling and CI support. The toolchain that grew around that decision used several runtimes and tools for one small frontend: Node for Vite, Vitest, `tsc`, Biome's launcher and the Tauri CLI; pnpm, bootstrapped through corepack; and Python for three helper scripts (`check-generated-bindings`, `read-rust-toolchain` and the native smoke journey). CI installed Node and pnpm in five jobs and Python in both Fedora containers.

Bun 1.4 can be the package manager, script runner, test runner, TypeScript checker and script runtime, and it runs on every host CI uses: Ubuntu, macOS, Windows x64 and the Fedora container. Each job was moved separately and checked for parity before it was adopted: a replacement had to check exactly as much as the tool it replaced, not just run faster.

## Decision
Use **Bun** (pinned in `.bun-version`; 1.4.4 at adoption) as the only JavaScript runtime and package manager for development, CI and packaging. Nothing JavaScript is shipped in the application, before or after.

| Job | Before | After |
| --- | --- | --- |
| Packages | pnpm 12, `pnpm-lock.yaml`, `pnpm-workspace.yaml` | `bun install`, `bun.lock`, `bunfig.toml` (`exact`, isolated linker, three-day `minimumReleaseAge`) |
| Lockfile release age | enforced by pnpm on every install | enforced by Bun when it resolves; `scripts/verify-lockfile-age.ts` re-checks `bun.lock` after every frozen install in CI |
| Scripts and Tauri hooks | `pnpm <script>`, `beforeBuildCommand: pnpm build` | `bun run <script>`, `beforeBuildCommand: bun run build` |
| Type check | `tsc --noEmit` | `bun check --no-script`, after `scripts/check-typescript-version.ts` |
| Tests | Vitest 5 on Node | `bun test --parallel` with jsdom ([ADR-0004](0004-frontend-testing-bun-test-rtl.md)) |
| Bundler and dev server | Vite on Node | Vite on Bun's runtime (`bun --bun vite`) |
| Lint and format | Biome, launched by Node | Biome, launched by Bun (`bun --bun biome`) |
| Helpers and native smoke | Python 3 | TypeScript run by Bun (`scripts/*.ts`, `scripts/native-smoke/`) |
| Audit | `pnpm audit` | `bun audit` |
| CI setup | `actions/setup-node`, corepack, pnpm store cache, Python | `oven-sh/setup-bun` (commit-pinned), a cache of Bun's package cache keyed on `bun.lock` |

No CI job installs Node.js or Python. Tools whose launcher starts with `#!/usr/bin/env node` are run with `--bun`, so they use Bun even on runner images that ship their own Node.

## Measured gains
Measured on a shared, heavily loaded machine with old and new runs interleaved, so the numbers are provisional: their direction matters more than their size.

| Task | Before | After |
| --- | --- | --- |
| Cold install, as a fresh CI runner pays it | pnpm 4.85 s, including its lockfile verification | Bun 1.03 s; 3.61 s with the release-age verifier |
| Warm install / no-op install | 0.45 s / 0.15 s | 0.11 s / 0.02 s |
| `format:check` / `lint` | 0.42 s / 0.48 s | 0.17 s / 0.25 s |
| `typecheck` | 1.42 s (`tsc`) | 0.30 s (guard and `bun check`) |
| `build` | 3.25 s | 1.48 s |
| Full `check` (format, lint, typecheck, test, build) | 14.54 s | 9.68 s |
| Frontend tests, fastest run at load 65 to 170 | 14.07 s warm, 11.81 s cold (Vitest on Node) | 5.63 s warm, 6.64 s cold |
| Dev server ready | 1.10 s | 0.88 s |
| Tauri `beforeBuildCommand` | 3.12 s | 1.59 s |
| Rust toolchain reader / generated-bindings check | 46 ms / 335 ms (Python) | 33 ms / 155 ms |
| Native smoke start-up | 0.132 s (Python) | 0.043 s; the journey itself is dominated by the application |

CI also no longer downloads Node, bootstraps pnpm through corepack (including pnpm's switch to the version recorded in its lockfile), or installs Python in the Fedora containers. The whole Linux pipeline passed locally with no `node`, `pnpm` or `python` on `PATH`: install, the release-age check, the TypeScript guard, `check`, the helper tests, the Rust bindings export, `bindings:check`, a Tauri debug build and the native smoke journey.

## Consequences
### Positive
- One pinned runtime replaces Node, pnpm, corepack and Python, in CI and on developer machines.
- Every frontend task above is faster, most of them by about two times or more.
- The supply-chain controls are kept: exact pins, a frozen lockfile in CI, a three-day minimum release age, an isolated `node_modules` layout that rejects undeclared imports, commit-pinned Actions, and Renovate updates for all of them. That includes `.bun-version` and the Bun Renovate itself runs to regenerate `bun.lock` (`constraints.bun` in `renovate.json`).

### Negative and trade-offs
- **Vite and Biome are kept.** Bun has no linter, and `bun fmt` would restyle the codebase. Bun's bundler cannot lower syntax to the ES2021/Safari 13 targets, ties Tailwind to the copy embedded in Bun, and its dev server is still experimental. Vite runs on Bun's runtime at parity: CSS and every other chunk are byte-identical, but Bun's transpiler rewrites the preload helper Vite inlines, so the main chunk is 2 bytes larger and has a different hash than a Node build.
- **A frozen install does not enforce the release age.** `bun install --frozen-lockfile` accepts whatever `bun.lock` pins, and `bun add --minimum-release-age 0` overrides `bunfig.toml` without a warning. `scripts/verify-lockfile-age.ts` closes the gap in CI, but it needs the npm registry. It exits 2 when it cannot check, and CI treats that as a failure to re-run, never as a pass.
- **`bun check` uses the TypeScript bundled in Bun**, not the `typescript` devDependency, which stays for editors. `scripts/check-typescript-version.ts` fails `typecheck` when the two differ, so Renovate updates Bun, `@types/bun` and `typescript` as one group. When Bun lags a TypeScript release, that group stays red until Bun catches up.
- **The tests depend on our own glue code.** Bun 1.4.4 has no DOM environment, so `src/test/dom-environment.ts` is preloaded to set up jsdom, and `src/test/mock-compat.ts` shims the `vi.fn`/`vi.spyOn` shorthands Bun implements differently. Both need re-checking on Bun and jsdom upgrades. Plain `bun test` shares one global across files and fails, so `bun run test` is the entry point. Coverage loses branch and statement counts (ADR-0004).
- `bun run --filter` gives scripts no terminal, so the interactive scripts (`dev`, `tauri`, `desktop:*`) use `bun run --cwd apps/desktop`.
- Bun 1.4.4 reported a script that failed to spawn (`E2BIG`) as a success. The script that triggered it is fixed, and script names must not shadow the binary they call.
- `oven-sh/setup-bun` verifies no checksum for the Bun it downloads, and falls back to the latest Bun when it cannot read `.bun-version`, so CI compares `bun --version` with the pin before installing.
- The Renovate validator runs on Bun without its native `re2` module, so it checks custom-manager regexes with JavaScript `RegExp`, which accepts some patterns that the hosted app's RE2 rejects.
- **Unverified on Windows** until the first `Package / Windows NSIS + MSI` and release runs:
  - setup-bun on `windows-latest`
  - `bun --bun vite` and `bun --bun biome` through Bun's Windows shims
  - the Tauri CLI's native module loaded by Bun, including `tauri build --bundles nsis,msi` and its signing hook
  - `bun check` with Windows paths
  - `bun test ./scripts` with Windows paths, CRLF output and junctions
  - the native smoke journey's Windows code, ported line by line from Python but run only on Linux. `Bun.which` does not search the current directory as `shutil.which` did.

- `Bun.spawn` has no Windows creation flags, and it always redirects the child's standard handles. The SMAPI installer needs a console and handles that are not redirected, and it inherits them from the application that starts it, so the smoke test starts the application through `CreateProcessW` (`bun:ffi`, `scripts/native-smoke/win32-process.ts`) with `CREATE_NEW_CONSOLE` and no redirection, as Python's `Popen` did. Under `Bun.spawn` the installer exited with `0xE0434352`, the exception `Console.ReadKey()` throws without a console. The direct start and `tauri-driver` use the same launcher with `CREATE_NEW_PROCESS_GROUP` and their output redirected to log files.

  The Fedora container path (setup-bun with `unzip`, and no Python) has also not yet run in CI.

## Alternatives Considered
- **Keep Node and pnpm (ADR-0003)**: no new glue code, but slower on every task above (by up to about five times), with three runtimes to pin and install.
- **Bun as the package manager only, with Node for everything else**: Vitest and `tsc` would stay unchanged, but most of the time saved comes from the script runner, `bun check` and `bun test`, and CI would still need Node.
- **Vitest on Bun's runtime (`bun --bun vitest run`) instead of `bun test`**: no test changes, but slower than `bun test` and outside Vitest's tested runtimes. See ADR-0004.
- **Bun's bundler and HTML dev server instead of Vite**: faster to bundle, but without syntax lowering, with Tailwind tied to Bun's embedded copy, and with a dev server that listens on `::1` only and injects an inline script the page's CSP blocks.
