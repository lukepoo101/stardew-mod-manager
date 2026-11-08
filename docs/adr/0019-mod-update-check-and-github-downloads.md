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
6. **An optional GitHub token raises the limit** (added 2026-11-08). The user
   can paste a fine-grained personal access token in Settings; one with
   public-repository read access and no permissions is enough. With it GitHub
   allows 5000 requests an hour, so a check reads up to 200 repositories
   (still four at a time, and still reusing releases read in the last 30
   minutes). A Test button calls `GET https://api.github.com/rate_limit` and
   shows the allowance.
   - The token is stored in the system keychain (the `keyring` crate: Windows
     Credential Manager, the macOS Keychain, or the Secret Service on Linux),
     never in the manager's database, settings, logs or error details. The
     window only ever learns whether one is saved; it is not sent back.
   - It is sent as `Authorization: Bearer` only on requests to the API's own
     scheme, host and port (`https://api.github.com`). File downloads use a
     different client and never carry it, even when the asset URL is on
     `github.com` and redirects to `objects.githubusercontent.com`. A redirect
     away from the API host drops it (reqwest removes `Authorization` when a
     redirect changes host, port or scheme; tests check this).
   - When GitHub answers 401, the read is made again without the token, the
     check says the token was rejected, and later reads leave it out (so the
     20-repository cap applies again) until the user saves a new one or a
     Test of the saved one succeeds.
7. **Update all is the batch review, fed by downloads.** It downloads each
   update the panel can download, one after another (the one-download-per-
   profile rule stands, so there is no batch command), discarding each
   prepared install as soon as it is read, exactly as the batch install of
   chosen archives does. A failed download (a file that is not the mod, the
   wrong size, no matching release file, GitHub unreachable) becomes that
   mod's line in the list, with the reason, and does not stop the others.
   Nothing is installed until the user confirms the list; each update is
   then a replacement of its own (settings kept, restore point first,
   rollback on failure), journaled together as one change in Activity.
   Closing the dialog while it downloads stops the downloads still to come.
8. **Where an update came from stays visible.** The repository, tag and file
   name recorded with the acquisition, and whether GitHub's checksum was
   matched, are read back for history: each install in Activity of a
   package downloaded from GitHub says "Downloaded from GitHub release ...",
   and the restore point saved before the replacement names the repository
   and tag in its label (within the 80-character limit, shortening the mods'
   names first) and gives the full line beside it. A package chosen from
   this computer gets neither, as before. Installs that are not
   replacements save no restore point, so only Activity shows them.
9. **Nothing waits forever.** smapi.io has 30 seconds and GitHub 15 to answer.
   A download has 20 seconds to connect and stops if nothing arrives for a
   minute. A download that cannot connect, or gets no answer in time, is
   tried once more after two seconds before it fails; nothing has arrived
   by then, so the retry cannot mix two downloads. One that stops part way
   is not retried. Downloads left behind by a stopped app are deleted at
   the next start.

## Consequences
### Positive
- The answer to "is there an update?" is the one SMAPI gives, for every site.
- No accounts, keys or new terms of service. Neither service needs the app to
  be registered. A GitHub token is optional and only raises a limit.
- Downloaded mods get the same archive checks, review, replace-with-rollback
  and provenance as local ones, and history says which release each came
  from.
- Many updates can be reviewed and installed in one pass, and one failing
  does not hold up the rest.

### Negative
- Most updates are on Nexus and still need a manual download. Nexus
  integration (an `nxm://` handler, single sign-on, app registration) is
  separate work.
- smapi.io's API is public but "not officially released" and may change.
- A GitHub release proves who uploaded it to that repository, not that the
  code is safe (see `trust-model.md`).
- On Linux a saved token needs a running Secret Service (GNOME Keyring,
  KWallet). Without one, the token cannot be saved and Settings says why;
  checks go on without a token.

## Alternatives Considered
- **Querying each site's API directly for update checks**: more requests,
  more keys and terms, and answers that could differ from SMAPI's.
- **Choosing the newest release file regardless of version**: wrong for
  repositories that hold several mods, and for authors whose GitHub releases
  lag behind Nexus.
- **Storing the token in the manager's settings**: a plain-text secret in a
  file that support bundles and backups may copy. The keychain keeps it out
  of the data folder.
- **Signing in with GitHub (OAuth device flow)**: needs a registered OAuth
  app, for no more than a pasted token gives.
- **A batch download command in the backend**: would need its own progress,
  cancellation and per-mod errors over IPC, all of which the batch review
  already has; downloading one after another from it keeps one path.
