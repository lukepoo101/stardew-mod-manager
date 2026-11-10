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

## Downloads from Nexus Mods

A file downloaded from Nexus Mods (ADR-0020) gets the same checks as a chosen
archive. Its size must be the size Nexus lists, and its MD5 is looked up on
Nexus: a file Nexus knows as some other file is refused, and one Nexus does
not recognise is offered with that said in the review. An update is refused
unless it holds the mod at the suggested version. As with GitHub, this shows
the file is what Nexus lists, not who wrote it or what it does.

## The Nexus API key

Until the manager is registered with Nexus, only a development build (the
`nexus-personal-key` feature) can use Nexus, with the user's personal API
key, which Nexus allows for testing and personal use. A release build never
reads, saves or sends one. The key is kept in the system keychain like the
GitHub token, never in the manager's files, logs or error details, and the
window never receives it back. It is sent only to `https://api.nexusmods.com`,
in a header marked sensitive, with redirects not followed. File downloads use
the address Nexus hands back, which carries its own short-lived permission,
and never carry the key; that address's query is removed from any error kept.
An `nxm://` link's one-file key stays in the backend: the window sees only
which mod and file the link names.

## What those checks do not establish

They say nothing about what the code does. A valid manifest, a matching
checksum, a known provider or a recognisable author is **not** evidence that a
mod is harmless, and the UI must never word it that way (no "safe mod" badge).

## Where the wording lives

`apps/desktop/src/shared/security/trust.ts` holds the user-facing text. Any new
surface that talks about mod safety imports it so the claim cannot drift. If a
real sandbox is ever added, revisit both that file and this document.
