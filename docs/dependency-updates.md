# Dependency updates

The hosted Renovate app reads `renovate.json`. Keep manager discovery open so
new dependency ecosystems are detected automatically. npm, Cargo, GitHub Actions,
the Fedora packaging image, Node, Rust and pnpm use built-in managers. The Biome
preset tracks its configuration schema; two narrow regex managers track the
Renovate validator and cargo-audit versions installed by dependency CI.

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
| Node runtime and Node types | Node.js for non-major updates | Non-major updates automerge |
| pnpm and dependency-check tools | Separate PRs | Stable non-major updates automerge |
| Actions exercised by PR CI | GitHub Actions | Non-major and digest updates automerge |
| Artifact upload/download and release publishing Actions | Artifact Actions together; publishing separate | Review |
| Fedora image | Same digest across CI and release | Digest updates automerge; Fedora version upgrades reviewed |
| npm/Cargo pre-1.0 minor updates, 0.0.x updates | Outside general routine groups | Review |
| Major upgrades, replacements and rollbacks | Separate from non-major updates | Review |
| Workspace lockfile maintenance | Both lockfiles together | Weekly automerge after full CI |
| Vulnerability fixes | Separate security PRs | Urgent human review |

Stable patches of `0.x` packages can automerge; minor changes to their `0.x`
line cannot. Cargo treats those line changes as potentially breaking. Keep
coupled package families together even when they span lockfiles. If a batch
fails because of one dependency, split that dependency into a temporary
dedicated group so the others can proceed.

Normal npm and crate updates wait three days after release. Node, Rust and
Action code updates wait seven days. Strict release-age filtering happens
before ordinary PR creation; immediate PR creation then starts CI. Existing
Action digest pins are exempt from the delay. Normal updates have no weekly
schedule; only lockfile maintenance runs Monday 00:00–06:59 in `Europe/London`.
The six-PR concurrency cap limits expensive native builds without an hourly cap.

## Reproducible installs and transitive coverage

Application npm dependencies are exact versions, initially pinned to the versions
already resolved in `pnpm-lock.yaml`. Cargo keeps compatible version ranges to
allow unification. Weekly lockfile maintenance refreshes compatible transitive
dependencies in both `pnpm-lock.yaml` and `Cargo.lock`.

CI uses frozen pnpm installs and locked Cargo tests/builds. All `desktop:build*`
scripts forward `--locked` to Tauri's Cargo runner, including the optional
WebDriver feature tested on Windows. Node comes from `.node-version`, Rust from
`rust-toolchain.toml`, and pnpm from `package.json#packageManager`. Actions are
pinned to commits and the Fedora image to a digest, with Renovate maintaining
those pins. OS packages and the rustup bootstrap are still supplied by their
live upstream repositories.

pnpm also enforces a strict three-day release age for dependency resolution,
including transitive packages. Renovate's urgent security-PR override does not
override pnpm. For a necessary younger fix, add only the affected version to
`minimumReleaseAgeExclude` in `pnpm-workspace.yaml`, explain the advisory in the
PR, and remove the exception after the release is old enough:

```yaml
minimumReleaseAgeExclude:
  - 'affected-package@1.2.3'
```

## Security checks and exceptions

`Dependencies / Audit + Renovate` validates the real Renovate configuration,
runs `pnpm audit`, and runs `cargo audit --deny unsound` on every PR and main
push, plus daily at 05:00 UTC. Vulnerabilities at any reported severity and new
unsound advisories fail the job. Unmaintained Rust crates remain visible
warnings: review their upstream replacement paths instead of suppressing them.
The uploaded Rust JSON report is generated outside the checkout so it includes
the advisories excepted by the repository's blocking audit.

Dependabot alerts remain enabled. Renovate vulnerability handling is enabled;
confirm the hosted app can read the alerts in the Dependency Dashboard. Security
PRs bypass ordinary schedules and release delays and initially need review.
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
`.github/workflows/dependency-health.yml` (44.132.4 at initial rollout):

```sh
npx --yes --package renovate@44.132.4 -- renovate-config-validator --strict --no-global renovate.json
```

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
