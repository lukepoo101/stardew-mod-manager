# Trust model for third-party mods

Stardew mods are third-party programs. Once installed they run with the
permissions of the user's account. Neither SMAPI nor this manager sandboxes
them, and the manager does not claim otherwise.

## What the manager checks

- The package is a well-formed archive and every entry stays inside the target
  folder (see ADR-0008 and the Windows file-namespace rules).
- Manifests parse and declare a UniqueID.
- A SHA-256 of the retained package is recorded so a later change is detectable.

## What those checks do not establish

They say nothing about what the code does. A valid manifest, a matching
checksum, a known provider or a recognisable author is **not** evidence that a
mod is harmless, and the UI must never word it that way (no "safe mod" badge).

## Where the wording lives

`apps/desktop/src/shared/security/trust.ts` holds the user-facing text. Any new
surface that talks about mod safety imports it so the claim cannot drift. If a
real sandbox is ever added, revisit both that file and this document.
