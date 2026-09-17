# ADR-0017: Windows Support Scope and Platform Decisions

## Status
Accepted

## Date
2026-09-20

## Context

ADR-0015 established that operating-system behaviour belongs in infrastructure adapters behind
application ports, but it was implemented for Linux only. The application compiled on Windows
without working there: the composition root built a Linux inspector, discovery searched Linux
Steam directories, the launcher refused to report process state on any non-Linux target, the SMAPI
log path was hard-coded to a Linux directory, and the generic desktop build script only produced an
RPM.

Adding Windows therefore raised decisions that are product decisions, not just implementation
details. This ADR records them so the next platform does not have to rediscover them.

## Decisions

### 1. Platform behaviour is selected at compile time in one place

`manager-infra/src/platform/` owns every host-specific adapter, and `HostPlatform::for_host()` is the
only function that selects by target operating system. Application services depend on ports and
never learn which host they run on. Platform-independent data - installation layouts, the Steam VDF
reader, the dependency-manifest reader - lives in `platform/shared` and is compiled everywhere, which
is what lets one host describe another host's installation without pretending to be it.

### 2. Package validation applies the Windows file namespace on every host

A ZIP entry is rejected when Windows could not store it: reserved device names, reserved characters,
trailing spaces or dots, alternate-data-stream colons, and two entries that are the same file under
case-insensitive comparison. This is enforced on Linux too, because the package is the same artifact
on both platforms and pre-extraction is the last point at which the decision is still the user's.

### 3. Game installation identity is a path key, not a path string

`manager-core::path_semantics` models each filesystem's comparison rules. Registration and discovery
compare identity keys, so `C:\Games\Stardew Valley`, `c:/games/stardew valley` and a junction alias are
one installation on Windows while remaining three distinct paths on a case-sensitive filesystem.

### 4. Process ownership is proved by identity, never by pid or filename alone

A tracked process is the one whose (pid, kernel creation timestamp, image path) triple still matches.
Recognising a running game by executable name is used only to *block* mutation, never to terminate:
termination requires an identity this session established, and after a restart an unprovable process
is refused with instructions to exit from the game menu.

### 5. Closing the manager never terminates the game

The game is started detached, with no job object and no `KILL_ON_JOB_CLOSE` limit. This is a product
requirement, and it is why Windows Job Objects are deliberately not used for lifecycle management.

### 6. Modded launch goes through SMAPI directly, not through Steam

The manager starts `StardewModdingAPI.exe --mods-path <profile>`. Launching through Steam would give
the user the Steam overlay, playtime tracking and achievements, but Steam cannot pass a
profile-specific mods path, and per-profile isolation is the core feature. Storefront-integrated
launch is a documented non-goal for this release rather than an oversight; the launch layout is
behind `GameRuntimePort`, so a storefront strategy can be added without touching `LaunchService`.

### 7. Supported Windows baseline

Windows 10 22H2 x64 and Windows 11 x64. WebView2 is already present on both; the installer keeps the
download-bootstrapper mode so a machine that lacks it is repaired rather than failing. Obsolete
Windows versions are out of scope, which removes a large amount of undocumented behaviour from the
support contract.

### 8. NSIS is the primary consumer download; MSI is secondary

A mod manager has no inherent need for a machine-wide installation, so the NSIS installer defaults
to a per-user installation. The MSI is produced for users and managed environments that require it.

### 9. Signing is part of the definition of done for a public release

Release artifacts are Authenticode signed with a trusted timestamp, and the release pipeline fails if
any artifact does not verify. Signing credentials only exist in the protected `release` environment,
never in a pull-request build.

### 10. CI must run on Windows, not merely compile for it

The Windows job runs the full Rust workspace test suite on `windows-latest` and then builds, installs,
starts and uninstalls the packaged application. A compile-only Windows job was the reason the
runtime gaps above went unnoticed.

### 11. First Windows release is parity with the intended Linux product

Steam discovery plus manually selected installations. `Storefront::Gog` already exists in the model
and the UI, but automatic GOG discovery is not implemented on any platform, so Windows support does
not depend on solving that separate product gap. A GOG adapter plugs into the same discovery port
when it is built.

## Consequences

### Positive

- The platform boundary is real rather than aspirational: Linux and Windows are both adapter sets
  behind the same ports, and macOS becomes another adapter rather than another rewrite.
- Archive and deployment safety no longer depends on which host happened to build the package.
- Linux also benefits: platform selection is one file, and the Linux process backend gained the same
  ownership semantics the Windows backend needed.

### Negative

- The Windows file namespace is stricter than Linux, so some archives that would have installed on
  Linux alone are now refused everywhere.
- Windows adds a second process-identity implementation and a Win32 surface that has to be reviewed
  and tested on the platform.

### Neutral

- Windows runtime acceptance is a manual checklist, not a CI claim. Until it is completed on
  Windows 10 22H2 and Windows 11, the README and SUPPORT describe the intended scope rather than a
  tested one.
