# Dependency updates

The hosted Renovate app reads `renovate.json`. Keep manager discovery open so
new dependency ecosystems are detected automatically. Built-in managers cover
npm packages (the `bun` manager, which reads both `package.json` files and
`bun.lock`), Cargo, GitHub Actions (including `oven-sh/setup-bun`), the Fedora
packaging image, Bun (`.bun-version`, and the `constraints.bun` that sets the
Bun Renovate itself runs) and Rust. The Biome preset tracks its configuration
schema; two narrow regex managers track the Renovate validator and cargo-audit
versions installed by dependency CI.

## Merge policy

Renovate requests GitHub native squash automerge for eligible updates. The
`main` ruleset requires a pull request, successful CI and CodeQL checks from the
GitHub Actions app, and an up-to-date branch. CodeQL results also block security
alerts at any severity and ordinary error-level alerts. It requires zero human approvals
and gives Renovate no bypass. Major updates still need a person to merge them:
Renovate does not request automerge for those PRs.

| Update | Group | Policy |
| --- | --- | --- |
| Stable npm minor/patch updates | npm dependencies, excluding dedicated groups | Automerge |
| Stable Cargo minor updates and compatible patches | Cargo dependencies, excluding Tauri | Automerge |
| Tauri core, CLI, API and plugins | Tauri stack across npm and Cargo | Compatible updates automerge; majors reviewed |
| Biome binary and schema | Biome | Non-major updates automerge |
| Bun (`.bun-version` and Renovate's own Bun), `@types/bun` and `typescript` | Bun and TypeScript | Non-major updates automerge once CI passes; see below |
| Dependency-check tools | Separate PRs | Stable non-major updates automerge |
| Actions exercised by PR CI | GitHub Actions | Non-major and digest updates automerge |
| Artifact upload/download Actions | Artifact Actions together | Non-major and digest updates automerge |
| Release publishing Action | Separate | Review |
| Fedora image | Same digest across CI and release | Digest updates automerge; Fedora version upgrades reviewed |
| npm/Cargo pre-1.0 minor updates | Outside general routine groups | Automerge |
| 0.0.x updates | Outside general routine groups | Review |
| Major upgrades, replacements and rollbacks | Separate from non-major updates | Review |
| Workspace lockfile maintenance | Both lockfiles together | Weekly automerge after full CI |
| Vulnerability fixes | Separate security PRs | Automerge after full CI |

Patches and minor changes of `0.x` packages automerge. Cargo treats `0.x` line
changes as potentially breaking, so they stay in their own PRs, and the full
native CI decides whether they merge. Keep
coupled package families together even when they span lockfiles. If a batch
fails because of one dependency, split that dependency into a temporary
dedicated group so the others can proceed.

Bun, `@types/bun` and `typescript` move together because `bun check`, the type
checker CI runs, uses the TypeScript bundled inside Bun rather than the
`typescript` package. `bun run typecheck` first runs
`scripts/check-typescript-version.ts`, which fails when the two differ, so a
PR that moves only one of them fails CI by design. When Bun and TypeScript
release out of step, the group PR stays red until a Bun release bundles the
new TypeScript. If one side is a major update and the other is not, Renovate
opens two PRs (majors are separated); combine them by hand. Keep the
`typescript` devDependency: editors use it.

Normal npm and crate updates wait three days after release. Bun, TypeScript, Rust
and Action code updates wait seven days. Strict release-age filtering happens
before ordinary PR creation; immediate PR creation then starts CI. Existing
Action digest pins are exempt from the delay. Normal updates have no weekly
schedule; only lockfile maintenance runs Monday 00:00–06:59 in `Europe/London`.
The ten-PR concurrency cap limits expensive native builds without an hourly cap.
Blocked PRs count against it, so a required check that fails everywhere stops
new updates from being opened at all.

## Reproducible installs and transitive coverage

Application npm dependencies are exact versions (`bunfig.toml` sets `exact = true`
for `bun add`, matching Renovate's `rangeStrategy: pin`). Packages are installed
with Bun's isolated linker, so each package sees only the dependencies it
declares. Cargo keeps compatible version ranges to allow unification. Weekly
lockfile maintenance refreshes compatible transitive dependencies in both
`bun.lock` and `Cargo.lock`. `constraints.bun` in `renovate.json` makes Renovate
regenerate `bun.lock` with the same Bun as `.bun-version`, and the Bun group
updates both together.

CI uses frozen Bun installs and locked Cargo tests/builds. All `desktop:build*`
scripts forward `--locked` to Tauri's Cargo runner, including the optional
WebDriver feature tested on Windows. Bun comes from `.bun-version` and Rust from
`rust-toolchain.toml`. Actions are
pinned to commits and the Fedora image to a digest, with Renovate maintaining
those pins. OS packages and the rustup bootstrap are still supplied by their
live upstream repositories.

Bun enforces a three-day release age (`minimumReleaseAge = 259200` seconds in
`bunfig.toml`) whenever it resolves a version, including transitive packages, so
`bun add` and `bun install` without a lockfile entry refuse anything younger. It
does **not** re-check versions already in `bun.lock`, and
`bun add --minimum-release-age 0` overrides the policy without a warning. CI
therefore runs `bun scripts/verify-lockfile-age.ts` straight after every frozen
install. It dates every `name@version` in `bun.lock` against the npm registry and
exits 1, naming each offending entry and when it becomes old enough, if any is
too young. It exits 2 when it could not check (registry unreachable, policy
missing, unreadable files); that is a failure to re-run, never a pass. There is
no skip flag.

Renovate's urgent security-PR override does not override Bun or the verifier.
For a necessary younger fix, add only the affected version to
`minimumReleaseAgeExcludes` in `bunfig.toml`, explain the advisory in the PR,
and remove the exception after the release is old enough. Bun and the verifier
accept a name or an exact `name@version`; a range such as `name@^1` is ignored
with a warning.

```toml
[install]
minimumReleaseAgeExcludes = ["affected-package@1.2.3"]
```

## Security checks and exceptions

`Dependencies / Audit + Renovate` validates the real Renovate configuration,
runs `bun audit`, and runs `cargo audit --deny unsound` on every PR and main
push, plus daily at 05:00 UTC. Vulnerabilities at any reported severity and new
unsound advisories fail the job. Unmaintained Rust crates remain visible
warnings: review their upstream replacement paths instead of suppressing them.
The uploaded Rust JSON report is generated outside the checkout so it includes
the advisories excepted by the repository's blocking audit.

Dependabot alerts remain enabled. Renovate vulnerability handling is enabled;
confirm the hosted app can read the alerts in the Dependency Dashboard. Security
PRs bypass ordinary schedules and release delays and automerge once full CI
passes.
Do not globally require `renovate/artifacts` or `renovate/stability-days`:
ordinary human PRs do not emit those bot-specific checks.

### glib: RUSTSEC-2024-0429

Reviewed 2026-10-03. `glib 0.18.5` is affected by
[RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html), also
[GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g).
The `Variant` API can violate Rust's safety guarantees. This remains an upstream
issue, not a fixed dependency or an established unreachable code path.

The Linux GTK3/WebKitGTK dependency stack reaches `glib 0.18` through `gtk`,
`tauri`, `tao`, `wry` and `muda`. It cannot accept `glib >=0.20`, the fixed line,
through a compatible lockfile update. The other, newer `glib` copy in the graph
does not remove the affected copy. A parent-stack migration is required.

`.cargo/audit.toml` excepts only this advisory so new issues can be gated now.
Keep [Dependabot alert #1](https://github.com/lukepoo101/stardew-mod-manager/security/dependabot/1)
open. Recheck on Tauri/GTK stack upgrades, any upstream backport, and before
releases. Remove the exception when the affected copy leaves the graph or a
compatible fixed release is adopted. Use this command to inspect its parents:

```sh
cargo tree --locked --target all -i glib@0.18.5
```

The implementation also patched `rustls` to `0.23.45` for
[RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285.html).

## Changing the policy

Run the validator after edits, using the pinned version from
`.github/workflows/dependency-health.yml` (44.198.2 when Bun was adopted):

```sh
bunx --bun --ignore-scripts --minimum-release-age 259200 --package renovate@44.198.2 renovate-config-validator --strict --no-global renovate.json
```

`--ignore-scripts` skips building Renovate's optional native `re2` module, which
Bun cannot load. The validator then checks regular expressions with JavaScript
`RegExp` and prints `RE2 not usable, falling back to RegExp`. The hosted app uses
RE2, which rejects some constructs `RegExp` accepts, such as lookaheads, so keep
custom-manager patterns to RE2 syntax.

Successful schema/preset validation does not prove update or merge behavior.
Check the first Renovate PRs and lockfile refresh against the required native
package jobs. No privileged approval workflow or merge queue is needed for
this policy. CI and dependency checks accept `merge_group`; verify CodeQL's
queue support too before enabling a queue later.

The policy follows mechanisms recommended by
[Renovate best practices](https://docs.renovatebot.com/presets-config/#configbest-practices),
[automerge guidance](https://docs.renovatebot.com/key-concepts/automerge/), and
[minimum release age](https://docs.renovatebot.com/key-concepts/minimum-release-age/).
Public projects differ on grouping and delays; these groups reflect this
application's coupled Tauri stack and native CI coverage.
