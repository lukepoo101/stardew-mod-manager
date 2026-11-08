# ADR-0019: Mod Update Checks via smapi.io and Downloads from GitHub Releases

## Status
Accepted

## Date
2026-11-06

## Context
Players want to know which installed mods have updates and to install them
without leaving the manager. A mod's manifest names where it is published
(`UpdateKeys` such as `Nexus:1915` or `GitHub:owner/repo`), and SMAPI already
answers "is there a newer version?" for every site through its web API at
smapi.io, the same check it runs when the game starts. That API also returns
the community compatibility list's status for the mod and, often, its GitHub
repository.

Where the file can be downloaded from is a separate question:

- **Nexus Mods** hosts most Stardew mods, but downloading through its API
  needs a registered application, a user API key, Premium for one-click
  downloads, and an `nxm://` handler for everyone else.
- **CurseForge** needs an API key granted by application, and each author can
  refuse third-party downloads.
- **ModDrop** has no public API.
- **GitHub releases** need no account. Many authors publish ZIPs there, and
  GitHub records a SHA-256 for files uploaded since mid-2025.

## Decision
1. **The update check is smapi.io's.** `POST https://smapi.io/api/v4.0.0/mods`
   with each mod's UniqueID, installed version and update keys, the SMAPI and
   game versions, and `includeExtendedMetadata`. It runs only when the user
   asks, never in the background. The last result is kept per profile and
   matched to what is installed now (an update already installed is no longer
   shown, and a suggested version that is not newer than the installed one,
   such as a release behind an installed beta, is not an update). A failed
   check, or an answer that is not a list of the mods asked about, keeps the
   last result, and the failure is stored with it so the page keeps saying
   why.
2. **Downloads come only from GitHub releases**, from the repository the mod's
   own `GitHub:` update key names, or else the one the compatibility list
   gives. Everything else gets an "Open mod page" link to the page smapi.io
   names. The manager opens only https pages on the sites mods are published
   on (Nexus Mods, GitHub, ModDrop, CurseForge, smapi.io, the wiki and the
   forums), with no user name, password or port.
3. **Nothing is guessed** (`manager_core::updates::choose_release_asset`). A
   release file is offered only when it is a ZIP in a published (non-draft)
   release whose tag, release name or file name carries exactly the suggested
   version ("2.0" and "2.0.0" are one version; "1.2.3a", "1.2.3-dev" and a
   GitHub prerelease are not "1.2.3"). Names are compared as whole words, so
   "Fluent" is not "Project Fluent", and source-code and symbol files beside
   the mod are ignored. With a subkey (`GitHub:owner/repo@Name`) only files named for it
   count. When the repository came from the compatibility list, or there are
   several candidates, the file or tag must also be named for the mod, because
   one repository often holds many mods' releases. Files over 256 MB are not
   downloaded, and a download stops as soon as it passes the size GitHub
   published, and is refused unless it ends at exactly that size.
4. **A download goes through the normal install pipeline.** GitHub's SHA-256
   is checked when there is one. The file is stored by its checksum, with a
   `Provider` acquisition recording the repository, tag and file URL. It is
   inspected and staged like a chosen archive, and the user reviews and
   confirms the replacement in the same dialog. The prepared install is
   discarded unless the archive holds the mod's UniqueID at the suggested
   version. If the old version came from an archive holding other mods that
   the new one does not contain, the review names them, since they go with
   the old archive. A mod that was turned off stays off.
5. **Limits are respected.** One check reads at most 20 GitHub repositories,
   four at a time, each once however many of its mods have updates, so a
   check stays within GitHub's 60 requests an hour for requests without an
   account. Releases read in the last 30 minutes are reused and do not count,
   so the next check reads the repositories left out. A mod whose repository
   was not read, or could not be, still offers Download, which reads it then.
   Only one download per profile runs at a time, and a new one closes the
   review an earlier one left open.
6. **Nothing waits forever.** smapi.io has 30 seconds and GitHub 15 to answer.
   A download has 20 seconds to connect and stops if nothing arrives for a
   minute. Downloads left behind by a stopped app are deleted at the next
   start.

## Consequences
### Positive
- The answer to "is there an update?" is the one SMAPI gives, for every site.
- No accounts, keys or new terms of service. Neither service needs the app to
  be registered.
- Downloaded mods get the same archive checks, review, replace-with-rollback
  and provenance as local ones.

### Negative
- Most updates are on Nexus and still need a manual download. Nexus
  integration (an `nxm://` handler, single sign-on, app registration) is
  separate work.
- smapi.io's API is public but "not officially released" and may change.
- A GitHub release proves who uploaded it to that repository, not that the
  code is safe (see `trust-model.md`).

## Alternatives Considered
- **Querying each site's API directly for update checks**: more requests,
  more keys and terms, and answers that could differ from SMAPI's.
- **Choosing the newest release file regardless of version**: wrong for
  repositories that hold several mods, and for authors whose GitHub releases
  lag behind Nexus.
