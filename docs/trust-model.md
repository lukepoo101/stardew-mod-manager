# Trust model for third-party mods

Stardew mods are third-party programs. Once installed they run with the
permissions of the user's account. Neither SMAPI nor this manager sandboxes
them, and the manager does not claim otherwise.

## What the manager checks

- The package is a well-formed archive and every entry stays inside the target
  folder (see ADR-0008 and the Windows file-namespace rules).
- Manifests parse and declare a UniqueID.
- A SHA-256 of the retained package is recorded so a later change is detectable.

## Downloaded updates

An update downloaded from a GitHub release (ADR-0019) gets the same checks as
a chosen archive. It is also checked against GitHub's published SHA-256 when
there is one, and refused unless it holds the mod it was downloaded for at the
suggested version. A download that grows past the size GitHub published is
stopped, and one that ends at another size is refused. Those checks show the file is what the release published.
They do not show who wrote it or what it does, and the review says the
publisher is not verified.

## The GitHub token

A GitHub token the user saves (ADR-0019) only raises GitHub's request limit
for reading public release lists; it grants nothing over what can be read
without one, and a fine-grained token with no permissions is all the manager
asks for. It is kept in the system keychain, not in the manager's files,
logs or error details, and the window never receives it back. It is sent
only to `https://api.github.com`: never with a file download, and dropped
if a request is redirected to another host. A token GitHub rejects stops
being sent until it is replaced. A token does not make a release more
trustworthy: the checks above are the same with or without one.

## What those checks do not establish

They say nothing about what the code does. A valid manifest, a matching
checksum, a known provider or a recognisable author is **not** evidence that a
mod is harmless, and the UI must never word it that way (no "safe mod" badge).

## Where the wording lives

`apps/desktop/src/shared/security/trust.ts` holds the user-facing text. Any new
surface that talks about mod safety imports it so the claim cannot drift. If a
real sandbox is ever added, revisit both that file and this document.
